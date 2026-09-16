//! Native lifecycle notifications are independent of renderer activity/timers.
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn request_lock(reason: &str) {
    log::debug!("Profile lock requested: {reason}");
    let Some(handle) = APP.get() else {
        return;
    };
    show_native_cover();
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
    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
        #[cfg(target_os = "ios")]
        {
            let _ = objc2_foundation::NSNotificationCenter::defaultCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(&NSString::from_str(
                        "UIApplicationWillResignActiveNotification",
                    )),
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

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_teymz_wealthfolio_MainActivity_lockProfile(
    _env: jni::JNIEnv,
    _class: jni::objects::JObject,
) {
    request_lock("Android background");
}

static COVER_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ACTIVE_COVER: std::sync::Mutex<u64> = std::sync::Mutex::new(0);

fn show_native_cover() {
    use std::sync::atomic::Ordering;
    let epoch = COVER_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Ok(mut active) = ACTIVE_COVER.lock() else {
        return;
    };
    *active = epoch;
    drop(active);
    log::debug!("Native privacy cover requested, transition {epoch}");
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let Some(window) = APP.get().and_then(|h| h.get_webview_window("main")) {
        if let Err(error) = window.with_webview(move |webview| {
            let Ok(active) = ACTIVE_COVER.lock() else {
                return;
            };
            if *active != epoch {
                return;
            }
            // SAFETY: Tauri invokes this closure on the native UI thread; inner
            // is the live WKWebView. All native cover ownership stays here.
            unsafe {
                apple_cover::show(webview.inner().cast());
            }
            log::debug!("Native privacy cover shown, transition {epoch}");
        }) {
            log::error!("Unable to show native privacy cover: {error}");
        }
    }
}

#[tauri::command]
pub fn profile_cover_state() -> Result<u64, String> {
    ACTIVE_COVER
        .lock()
        .map(|v| *v)
        .map_err(|_| "Privacy cover unavailable".into())
}

#[tauri::command]
pub fn profile_cover_ready(epoch: u64) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let window = APP
            .get()
            .and_then(|h| h.get_webview_window("main"))
            .ok_or("Privacy window unavailable")?;
        window
            .with_webview(move |_| {
                let Ok(mut active) = ACTIVE_COVER.lock() else {
                    return;
                };
                if !accept_cover_ack(*active, epoch) {
                    return;
                }
                // The renderer acknowledged a safe frame for this exact cover.
                unsafe {
                    apple_cover::remove();
                }
                *active = 0;
                log::debug!("Native privacy cover removed, transition {epoch}");
            })
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "android")]
    {
        let window = APP
            .get()
            .and_then(|h| h.get_webview_window("main"))
            .ok_or("Privacy window unavailable")?;
        window
            .with_webview(move |webview| {
                webview.jni_handle().exec(move |env, activity, _| {
                    let Ok(mut active) = ACTIVE_COVER.lock() else {
                        return;
                    };
                    if !accept_cover_ack(*active, epoch) {
                        return;
                    }
                    match env.call_method(activity, "onProfileCoverReady", "()V", &[]) {
                        Ok(_) => *active = 0,
                        Err(error) => {
                            log::error!("Unable to acknowledge Android privacy cover: {error}")
                        }
                    }
                });
            })
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    {
        let mut active = ACTIVE_COVER
            .lock()
            .map_err(|_| "Privacy cover unavailable")?;
        if accept_cover_ack(*active, epoch) {
            *active = 0;
        }
    }
    Ok(())
}

fn accept_cover_ack(active: u64, acknowledged: u64) -> bool {
    active != 0 && active == acknowledged
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_cover {
    use super::APP;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, define_class, msg_send, sel, ClassType, MainThreadOnly};
    use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use tauri::Manager;

    define_class!(
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct CoverActions;
        unsafe impl NSObjectProtocol for CoverActions {}
        impl CoverActions {
            #[unsafe(method(retryProfile:))]
            fn retry(&self, _sender: &AnyObject) {
                if let Some(window) = APP.get().and_then(|h| h.get_webview_window("main")) {
                    // A fresh document is safe: all previous grants were revoked.
                    // Keep the native cover until that document acknowledges it.
                    if let Err(error) = window.eval("if (window.dispatchEvent(new Event('wealthfolio:before-reload', { cancelable: true }))) window.location.reload()") {
                        log::error!("Unable to reload locked renderer: {error}");
                    }
                }
            }
        }
    );
    thread_local! {
        static COVER: RefCell<Option<(Retained<AnyObject>, Retained<CoverActions>)>> = const { RefCell::new(None) };
    }

    pub unsafe fn remove() {
        COVER.with(|slot| {
            if let Some((view, _)) = slot.borrow_mut().take() {
                let _: () = msg_send![&view, removeFromSuperview];
            }
        });
    }

    pub unsafe fn show(webview: *mut AnyObject) {
        // Reuse an existing opaque surface for repeated lifecycle notifications.
        if COVER.with(|slot| slot.borrow().is_some()) {
            return;
        }
        let bounds: NSRect = msg_send![webview, bounds];
        let actions: Retained<CoverActions> = msg_send![CoverActions::class(), new];
        #[cfg(target_os = "macos")]
        let view: Retained<AnyObject> = {
            let allocated: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSView), alloc];
            let view: Retained<AnyObject> = msg_send![allocated, initWithFrame: bounds];
            let _: () = msg_send![&view, setAutoresizingMask: 18usize]; // width + height
            let _: () = msg_send![&view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![&view, layer];
            // Match the dark index.html splash (#100f0f).
            let color: Retained<AnyObject> = msg_send![class!(NSColor), colorWithSRGBRed: 16.0f64 / 255.0, green: 15.0f64 / 255.0, blue: 15.0f64 / 255.0, alpha: 1.0f64];
            let appearance_name = NSString::from_str("NSAppearanceNameDarkAqua");
            let appearance: Retained<AnyObject> =
                msg_send![class!(NSAppearance), appearanceNamed: &*appearance_name];
            let _: () = msg_send![&view, setAppearance: &*appearance];
            let cg_color: *const std::ffi::c_void = msg_send![&color, CGColor];
            let _: () = msg_send![layer, setBackgroundColor: cg_color];
            view
        };
        #[cfg(target_os = "ios")]
        let view: Retained<AnyObject> = {
            let allocated: objc2::rc::Allocated<AnyObject> = msg_send![class!(UIView), alloc];
            let view: Retained<AnyObject> = msg_send![allocated, initWithFrame: bounds];
            let _: () = msg_send![&view, setAutoresizingMask: 18usize];
            let color: Retained<AnyObject> = msg_send![class!(UIColor), colorWithRed: 16.0f64 / 255.0, green: 15.0f64 / 255.0, blue: 15.0f64 / 255.0, alpha: 1.0f64];
            let _: () = msg_send![&view, setBackgroundColor: &*color];
            let _: bool = msg_send![webview, endEditing: true];
            view
        };
        // Embed the same asset as the web splash; no filesystem or renderer dependency.
        let logo_bytes = include_bytes!("../../frontend/public/logo-gold.png");
        let data: Retained<AnyObject> = msg_send![class!(NSData), dataWithBytes: logo_bytes.as_ptr().cast::<std::ffi::c_void>(), length: logo_bytes.len()];
        let title = NSString::from_str("Reload lock screen");
        let logo_frame = NSRect::new(
            NSPoint::new(
                (bounds.size.width - 80.0) / 2.0,
                (bounds.size.height - 80.0) / 2.0,
            ),
            NSSize::new(80.0, 80.0),
        );
        #[cfg(target_os = "macos")]
        let button_y = bounds.size.height / 2.0 - 104.0;
        #[cfg(target_os = "ios")]
        let button_y = bounds.size.height / 2.0 + 68.0;
        let button_frame = NSRect::new(
            NSPoint::new((bounds.size.width - 200.0) / 2.0, button_y),
            NSSize::new(200.0, 36.0),
        );
        #[cfg(target_os = "macos")]
        let (logo, button): (Retained<AnyObject>, Retained<AnyObject>) = {
            let allocated: objc2::rc::Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let image: Retained<AnyObject> = msg_send![allocated, initWithData: &*data];
            let logo: Retained<AnyObject> = msg_send![class!(NSImageView), new];
            let _: () = msg_send![&logo, setImage: &*image];
            let _: () = msg_send![&logo, setImageScaling: 3usize]; // proportional aspect fit
            let description = NSString::from_str("Wealthfolio");
            let _: () = msg_send![&logo, setAccessibilityLabel: &*description];
            let button: Retained<AnyObject> = msg_send![class!(NSButton), buttonWithTitle: &*title, target: &*actions, action: sel!(retryProfile:)];
            (logo, button)
        };
        #[cfg(target_os = "ios")]
        let (logo, button): (Retained<AnyObject>, Retained<AnyObject>) = {
            let image: Retained<AnyObject> = msg_send![class!(UIImage), imageWithData: &*data];
            let logo: Retained<AnyObject> = msg_send![class!(UIImageView), new];
            let _: () = msg_send![&logo, setImage: &*image];
            let _: () = msg_send![&logo, setContentMode: 1isize]; // aspect fit
            let description = NSString::from_str("Wealthfolio");
            let _: () = msg_send![&logo, setIsAccessibilityElement: true];
            let _: () = msg_send![&logo, setAccessibilityLabel: &*description];
            let button: Retained<AnyObject> = msg_send![class!(UIButton), buttonWithType: 1isize];
            let _: () = msg_send![&button, setTitle: &*title, forState: 0usize];
            let _: () = msg_send![&button, addTarget: &*actions, action: sel!(retryProfile:), forControlEvents: 64usize];
            (logo, button)
        };
        let _: () = msg_send![&logo, setFrame: logo_frame];
        let _: () = msg_send![&button, setFrame: button_frame];
        let _: () = msg_send![&logo, setAutoresizingMask: 45usize]; // centered margins
        let _: () = msg_send![&button, setAutoresizingMask: 45usize];
        let _: () = msg_send![&view, addSubview: &*logo];
        let _: () = msg_send![&view, addSubview: &*button];
        // Do not hide WKWebView: it must keep rendering the safe lock surface.
        let _: () = msg_send![webview, addSubview: &*view];
        COVER.with(|slot| *slot.borrow_mut() = Some((view, actions)));
    }
}

#[cfg(test)]
mod cover_tests {
    use super::accept_cover_ack;
    #[test]
    fn only_the_current_nonzero_cover_can_be_acknowledged() {
        assert!(!accept_cover_ack(2, 1));
        assert!(!accept_cover_ack(0, 0));
        assert!(!accept_cover_ack(0, 2));
        assert!(accept_cover_ack(2, 2));
    }
}
