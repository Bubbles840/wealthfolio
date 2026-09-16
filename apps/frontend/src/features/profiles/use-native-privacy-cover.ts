import { useEffect, useState } from "react";
import { isWeb, logger } from "@/adapters";
import { profileCommand } from "./api";

/** Acknowledge only a committed non-financial screen for the current cover. */
export function useNativePrivacyCover(safe: boolean) {
  const [epoch, setEpoch] = useState(0);
  useEffect(() => {
    if (isWeb) return;
    let cancelled = false;
    let reading = false;
    const refresh = async () => {
      if (reading) return;
      reading = true;
      try {
        const next = await profileCommand<number>("profile_cover_state");
        if (!cancelled) setEpoch(next);
      } catch {
        logger.error("Unable to read native privacy cover state.");
      } finally {
        reading = false;
      }
    };
    void refresh();
    const timer = window.setInterval(refresh, 1000);
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) => listen("profile-session-changed", () => void refresh()))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        logger.error("Unable to listen for native privacy cover changes.");
      });
    return () => {
      cancelled = true;
      clearInterval(timer);
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (isWeb || !safe || !epoch) return;
    let cancelled = false;
    let frame = requestAnimationFrame(() => {
      // Leave the native surface above the first safe frame's paint.
      frame = requestAnimationFrame(() => {
        if (cancelled) return;
        void profileCommand("profile_cover_ready", { epoch }).catch(() => {
          logger.error("Unable to dismiss native privacy cover; retrying.");
          if (!cancelled) setEpoch(0);
        });
      });
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [safe, epoch]);
}
