import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ProfileAvatar } from "@/features/profiles/profile-avatar";
import { useProfile } from "@/features/profiles/profile-context";
import { openingProfileHint } from "@/features/profiles/startup-hint";

interface StartupScreenProps {
  profile?: { name: string; avatarId: string } | null;
  message?: string;
  error?: string;
  children?: ReactNode;
}

/** This fallback must render even before translation resources are ready. */
export function StartupScreen({ profile, message, error, children }: StartupScreenProps) {
  const { t } = useTranslation("common", { useSuspense: false });
  const context = useProfile();
  const identity = profile === undefined ? (context?.profile ?? openingProfileHint()) : profile;
  const label =
    message ??
    (identity
      ? t("profiles.openingProfile", { name: identity.name, defaultValue: "Opening {{name}}…" })
      : t("profiles.opening", { defaultValue: "Opening Wealthfolio…" }));
  return (
    <main
      className={`flex min-h-dvh flex-col items-center justify-center gap-6 px-6 py-16 text-center ${identity ? "bg-background text-foreground" : "bg-[#100f0f] text-[#fffcf0]"}`}
    >
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-8" />
      {identity ? (
        <ProfileAvatar id={identity.avatarId} />
      ) : (
        <img
          src="/logo-gold.png"
          alt="Wealthfolio"
          className={`size-20 object-contain ${error ? "" : "animate-[spin_3s_linear_infinite] motion-reduce:animate-none"}`}
        />
      )}
      <p role="status" className={error ? "break-words text-lg font-medium" : "sr-only"}>
        {label}
      </p>
      {error && (
        <div className="w-full max-w-sm space-y-3">
          <p role="alert" className="text-destructive break-words text-sm">
            {error}
          </p>
        </div>
      )}
      {children && <div className="flex flex-wrap justify-center gap-3">{children}</div>}
    </main>
  );
}
