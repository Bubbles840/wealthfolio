//! Native lifecycle notifications are independent of renderer activity/timers.
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn request_lock(reason: &str) {
    log::debug!("Profile lock requested: {reason}");
    let Some(handle) = APP.get() else {
        return;
    };
    let root = handle.state::<crate::profiles::NativeProfiles>();
    let _ = root.registry.sessions.revoke(crate::profiles::NATIVE_OWNER);
    if let Some(runtime) = root.active().ok().flatten() {
        runtime.suspend();
    }
    let _ = handle.emit(crate::profiles::PROFILE_CHANGED, ());
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let _ = handle
            .state::<crate::profiles::NativeProfiles>()
            .lock(&handle)
            .await;
    });
}

pub fn install(handle: &AppHandle) {
    let _ = APP.set(handle.clone());
    #[cfg(target_os = "macos")]
    unsafe {
        use block2::RcBlock;
        use objc2_foundation::NSString;
        let block = RcBlock::new(|_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
            request_lock("native lifecycle")
        });
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::{
                NSWorkspace, NSWorkspaceSessionDidResignActiveNotification,
                NSWorkspaceWillSleepNotification,
            };
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            // The notification center retains these process-lifetime observers.
            let _ = center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillSleepNotification),
                None,
                None,
                &block,
            );
            let _ = center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceSessionDidResignActiveNotification),
                None,
                None,
                &block,
            );
            let distributed = objc2_foundation::NSDistributedNotificationCenter::defaultCenter();
            let _ = distributed.addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str("com.apple.screenIsLocked")),
                None,
                None,
                &block,
            );
        }
    }
    #[cfg(target_os = "windows")]
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            use windows_sys::Win32::System::RemoteDesktop::*;
            unsafe {
                let mut buffer = std::ptr::null_mut();
                let mut size = 0;
                if WTSQuerySessionInformationW(
                    std::ptr::null_mut(),
                    WTS_CURRENT_SESSION,
                    WTSSessionInfoEx,
                    &mut buffer,
                    &mut size,
                ) != 0
                {
                    if size as usize >= std::mem::size_of::<WTSINFOEXW>() {
                        let info = &*(buffer as *const WTSINFOEXW);
                        if info.Level == 1
                            && info.Data.WTSInfoExLevel1.SessionFlags
                                == WTS_SESSIONSTATE_LOCK as i32
                        {
                            request_lock("Windows session lock");
                        }
                    }
                    WTSFreeMemory(buffer.cast());
                }
            }
        }
    });
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    tauri::async_runtime::spawn(async {
        let Ok(connection) = zbus::Connection::system().await else {
            return;
        };
        let Ok(proxy) = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
        )
        .await
        else {
            return;
        };
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if proxy
                .get_property::<bool>("LockedHint")
                .await
                .unwrap_or(false)
            {
                request_lock("Linux session lock");
            }
        }
    });
}
