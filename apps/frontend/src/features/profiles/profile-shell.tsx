import { DATABASE_STATE_CHANGED } from "../../adapters/tauri/events";
import { clearProfilePreferences } from "@/hooks/use-persistent-state";
import { DeleteProfileDialog } from "./delete-profile-dialog";
import { StartupScreen } from "@/components/startup-screen";
import { reloadApplication } from "@/lib/reload-application";
import { clearOpeningProfile, rememberOpeningProfile } from "./startup-hint";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { isNativeAuthPending } from "./auth-bridge";
import { isWeb } from "@/adapters";
import { Button, Input, Label } from "@wealthfolio/ui";
import { PasswordInput } from "@wealthfolio/ui/components/ui/password-input";
import { useCallback, useEffect, useRef, useState, type ReactNode, type FormEvent } from "react";
import { profileCommand, type ProfileState, type ProfileSummary } from "./api";
import { DEFAULT_PROFILE_AVATAR, ProfileAvatar } from "./profile-avatar";
import { ProfileAvatarPicker } from "./profile-avatar-picker";
import "./profile-shell.css";
import { ProfileContext } from "./profile-context";
import {
  installProfileSession,
  profileScope,
  revokeProfileSession,
  type ProfileSession,
} from "./session";

const MIN_PASSWORD_LENGTH = 8;
const MAX_PASSWORD_LENGTH = 128;

export function ProfileShell({ children }: { children: ReactNode }) {
  const queries = useQueryClient();
  const { t } = useTranslation("common", { useSuspense: false });
  type Phase = "loading" | "closing" | "opening" | "locked" | "active" | "deleting";
  const [phase, updatePhase] = useState<Phase>("loading");
  const phaseRef = useRef<Phase>("loading");
  const setPhase = useCallback((next: Phase) => {
    phaseRef.current = next;
    updatePhase(next);
  }, []);
  const intent = useRef<"lock" | "switch">("lock");
  const epoch = useRef(0);
  const working = useRef(false);
  const currentProfile = useRef<ProfileSummary | undefined>(undefined);
  const refreshRef = useRef<(() => void) | undefined>(undefined);
  const needsClose = useRef(false);
  const [closeFailed, setCloseFailed] = useState(false);
  const [state, setState] = useState<ProfileState>();
  const [selected, setSelected] = useState<ProfileSummary>();
  const heading = useRef<HTMLHeadingElement>(null);
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [confirmationInvalid, setConfirmationInvalid] = useState(false);
  const confirmationInput = useRef<HTMLInputElement>(null);
  const [passwordInvalid, setPasswordInvalid] = useState(false);
  const passwordInput = useRef<HTMLInputElement>(null);
  const [proof, setProof] = useState("");
  const [passwordManagement, setPasswordManagement] = useState(false);
  const [removePassword, setRemovePassword] = useState(false);
  const [name, setName] = useState("");
  const [avatar, setAvatar] = useState(DEFAULT_PROFILE_AVATAR);
  const [mode, setMode] = useState<"choose" | "create" | "unlock" | "manage" | "recover">("choose");
  const [error, setError] = useState("");
  const [deleteOpen, setDeleteOpen] = useState(false);
  const deletion = useRef<{ profileId: string; confirmation: string } | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [recovery, setRecovery] = useState<string>();
  const [covered, setCovered] = useState(false);
  const sessionScope = state?.session?.scopeId;
  const [authPending, setAuthPending] = useState(isNativeAuthPending);
  const lock = useCallback(
    async (preserveAuth = false, nextIntent: "lock" | "switch" = "lock") => {
      if (working.current) return;
      working.current = true;
      intent.current = nextIntent;
      epoch.current += 1;
      setCloseFailed(false);
      setError("");
      setBusy(true);
      setPhase("closing");
      setCovered(true);
      setSelected(currentProfile.current);
      setPassword("");
      setConfirmPassword("");
      setConfirmationInvalid(false);
      revokeProfileSession();
      try {
        await profileCommand("lock_profile", { preserveAuth });
        needsClose.current = false;
        setState((old) => (old ? { ...old, session: null } : old));
        setMode("choose");
        setPhase("locked");
      } catch (e) {
        setError(String(e));
        setCloseFailed(true);
      } finally {
        working.current = false;
        setBusy(false);
      }
    },
    [setPhase],
  );
  useEffect(() => {
    const update = () => setAuthPending(isNativeAuthPending());
    window.addEventListener("wealthfolio:auth-pending", update);
    return () => window.removeEventListener("wealthfolio:auth-pending", update);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let inFlight = false;
    let refreshPending = false;
    const refresh = async () => {
      if (cancelled) return;
      if (inFlight) {
        refreshPending = true;
        return;
      }
      if (
        working.current ||
        phaseRef.current === "opening" ||
        phaseRef.current === "closing" ||
        phaseRef.current === "deleting"
      )
        return;
      inFlight = true;
      const requestEpoch = epoch.current;
      try {
        const next = await profileCommand<ProfileState>("get_profile_state");
        if (cancelled || requestEpoch !== epoch.current || working.current) return;
        const previous = currentProfile.current;
        if (previous && !next.profiles.some((profile) => profile.id === previous.id)) {
          intent.current = "switch";
          if (!next.pendingDeletions?.some((profile) => profile.id === previous.id))
            clearProfilePreferences(previous.id);
        }
        if (next.session) {
          const profile = next.profiles.find((p) => p.id === next.session?.profileId);
          if (profile) rememberOpeningProfile(profile);
          if (!installProfileSession(next.session)) return;
          currentProfile.current = profile;
          setState(next);
          setCovered(false);
          setPhase("active");
          clearOpeningProfile();
        } else if (next.starting) {
          setState(next);
          setPhase("loading");
        } else {
          // Join native teardown before offering another activation. A replacement
          // recovery grant above must reload without being revoked again.
          if (needsClose.current || phaseRef.current === "active") {
            needsClose.current = false;
            setState(next);
            await lock(true, intent.current);
            return;
          }
          setState(next);
          setCovered(true);
          setPhase("locked");
          if (
            !currentProfile.current &&
            next.profiles.length === 1 &&
            intent.current !== "switch"
          ) {
            currentProfile.current = next.profiles[0];
            setSelected(next.profiles[0]);
            setMode("choose");
          }
        }
      } catch (e) {
        if (!cancelled && requestEpoch === epoch.current) setError(String(e));
      } finally {
        inFlight = false;
        if (refreshPending && !cancelled) {
          refreshPending = false;
          void refresh();
        }
      }
    };
    refreshRef.current = () => void refresh();
    // Web has no native session events and must observe changes from other clients.
    const webTimer = isWeb ? window.setInterval(() => void refresh(), 2000) : undefined;
    const locked = () => {
      epoch.current += 1;
      void queries.cancelQueries();
      queries.clear();
      setCovered(true);
      if (!working.current) {
        needsClose.current = true;
        setPhase("loading");
        setSelected(currentProfile.current);
        setMode("choose");
        void refresh();
      }
    };
    window.addEventListener("wealthfolio:profile-locked", locked);
    const unlisteners: (() => void)[] = [];
    if (!isWeb)
      void import("@tauri-apps/api/event")
        .then(async ({ listen }) => {
          for (const [event, handler] of [
            ["profile-session-changed", revokeProfileSession],
            ["app:ready", () => void refresh()],
            // Rebuilds can revoke the scoped database reader while maintenance
            // is running. The shell survives that reader and renews admission.
            [DATABASE_STATE_CHANGED, () => void refresh()],
          ] as const) {
            const unlisten = await listen(event, handler);
            if (cancelled) {
              unlisten();
              return;
            }
            unlisteners.push(unlisten);
          }
          // Subscribe before reading so startup or lock changes cannot be missed.
          void refresh();
        })
        .catch((cause) => {
          if (cancelled) return;
          // A state read cannot replace the missing lock listener. Keep the
          // shell closed until a reload can register all subscriptions again.
          cancelled = true;
          unlisteners.splice(0).forEach((unlisten) => unlisten());
          refreshRef.current = () => reloadApplication();
          setError(String(cause));
        });
    else void refresh();
    return () => {
      cancelled = true;
      window.clearInterval(webTimer);
      unlisteners.forEach((unlisten) => unlisten());
      window.removeEventListener("wealthfolio:profile-locked", locked);
    };
  }, [queries, lock, setPhase]);

  useEffect(() => {
    if (!sessionScope || covered) return;
    let last = 0;
    const activity = (event: Event) => {
      if (!event.isTrusted || Date.now() - last < 15000) return;
      last = Date.now();
      void profileCommand("profile_activity", { scopeId: profileScope() }, true).catch(() =>
        revokeProfileSession(),
      );
    };
    for (const event of ["pointerdown", "keydown", "touchstart", "wheel"])
      window.addEventListener(event, activity, { passive: true });
    return () => {
      for (const event of ["pointerdown", "keydown", "touchstart", "wheel"])
        window.removeEventListener(event, activity);
    };
  }, [sessionScope, covered, lock]);

  useEffect(() => {
    if (
      phase === "locked" &&
      (mode === "choose" || (mode === "unlock" && !selected?.lockEnabled))
    ) {
      heading.current?.focus();
    }
  }, [phase, mode, selected?.lockEnabled]);

  useEffect(() => {
    if (passwordInvalid && !busy) {
      passwordInput.current?.focus();
      passwordInput.current?.select();
    }
  }, [passwordInvalid, busy]);

  async function run(action: () => Promise<void>) {
    if (working.current) return;
    working.current = true;
    epoch.current += 1;
    setBusy(true);
    setError("");
    setPasswordInvalid(false);
    try {
      await action();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setPhase("locked");
    } finally {
      working.current = false;
      setBusy(false);
    }
  }
  function select(profile: ProfileSummary) {
    if (working.current) return;
    if (profile.id !== currentProfile.current?.id) intent.current = "switch";
    setSelected(profile);
    setPasswordInvalid(false);
    setPassword("");
    setConfirmPassword("");
    setConfirmationInvalid(false);
    setProof("");
    setError("");
    if (profile.lockEnabled) setMode("unlock");
    else void run(() => unlock(profile.id, ""));
  }
  async function unlock(id: string, unlockProof = password) {
    const target = state?.profiles.find((p) => p.id === id);
    if (target) setSelected(target);
    try {
      await profileCommand<ProfileSession>("unlock_profile", {
        profileId: id,
        proof: unlockProof || null,
      });
    } catch (error) {
      if (String(error).includes("PROFILE_PASSWORD_INVALID") && mode === "unlock") {
        setPasswordInvalid(true);
        return;
      }
      throw error;
    }
    if (target) rememberOpeningProfile(target);
    setPhase("opening");
    // A fresh JS world is required even when reopening the same profile.
    reloadApplication({ dashboard: intent.current === "switch" });
  }
  async function deleteProfile(
    confirmation: string,
    proof: string,
    retry = false,
    target = selected,
  ) {
    if (!target || working.current) return;
    setSelected(target);
    working.current = true;
    epoch.current += 1;
    setBusy(true);
    setError("");
    setDeleteOpen(false);
    setPhase("deleting");
    deletion.current = { profileId: target.id, confirmation };
    try {
      await profileCommand("delete_profile", { ...deletion.current, proof }, !retry);
      clearProfilePreferences(target.id);
      setDeleteOpen(false);
      revokeProfileSession();
      await queries.cancelQueries();
      queries.clear();
      clearOpeningProfile();
      const next = await profileCommand<ProfileState>("get_profile_state");
      deletion.current = undefined;
      currentProfile.current = undefined;
      setSelected(undefined);
      intent.current = "switch";
      needsClose.current = false;
      setState(next);
      setCovered(true);
      setMode("choose");
      setPhase("locked");
    } catch (cause) {
      setError(String(cause).replace(/^Error:\s*/, ""));
      // Validation errors leave the profile intact and allow correction.
      const next = await profileCommand<ProfileState>("get_profile_state").catch(() => undefined);
      if (next) setState(next);
      if (next?.profiles.some((profile) => profile.id === target.id)) {
        setDeleteOpen(true);
        setPhase("active");
      } else {
        setDeleteOpen(false);
        revokeProfileSession();
        setPhase("deleting");
      }
    } finally {
      working.current = false;
      setBusy(false);
    }
  }
  if (phase === "deleting")
    return (
      <StartupScreen profile={selected} message="Deleting profile…" error={error || undefined}>
        <Button
          disabled={busy}
          onClick={() => {
            const pending = deletion.current;
            if (pending) void deleteProfile(pending.confirmation, "", true);
          }}
        >
          {busy ? "Deleting…" : "Retry deletion"}
        </Button>
        {!busy && (
          <Button
            variant="ghost"
            onClick={() => {
              setError("");
              currentProfile.current = undefined;
              setSelected(undefined);
              intent.current = "switch";
              needsClose.current = false;
              setMode("choose");
              setCovered(true);
              setPhase("locked");
            }}
          >
            Back to profiles
          </Button>
        )}
      </StartupScreen>
    );

  async function submit(event: FormEvent) {
    event.preventDefault();
    const settingPassword =
      mode === "recover" ||
      (mode === "manage" &&
        passwordManagement &&
        !removePassword &&
        (Boolean(password) || Boolean(confirmPassword) || !selected?.lockEnabled));
    if (settingPassword) {
      const length = Array.from(password).length;
      if (length < MIN_PASSWORD_LENGTH || length > MAX_PASSWORD_LENGTH) {
        setError(`Use ${MIN_PASSWORD_LENGTH}–${MAX_PASSWORD_LENGTH} characters for your password.`);
        return;
      }
      if (password !== confirmPassword) {
        setError("");
        setConfirmationInvalid(true);
        confirmationInput.current?.focus();
        return;
      }
    }
    await run(async () => {
      if (mode === "unlock" && selected) await unlock(selected.id);
      if (mode === "create") {
        const created = await profileCommand<ProfileSummary>("create_profile", {
          name,
          avatarId: avatar,
        });
        intent.current = "switch";
        setSelected(created);
        rememberOpeningProfile(created);
        await unlock(created.id);
      }
      if (mode === "recover" && selected) {
        setRecovery(
          await profileCommand<string>("recover_profile_password", {
            profileId: selected.id,
            recoveryCode: proof,
            password,
          }),
        );
      }
      if (mode === "manage") {
        await profileCommand("update_profile", { name, avatarId: avatar }, true);
        if (passwordManagement && (password || removePassword)) {
          const code = await profileCommand<string | null>(
            "set_profile_password",
            { proof: proof || null, password: removePassword ? null : password || null },
            true,
          );
          setRecovery(code ?? undefined);
          revokeProfileSession();
          setCovered(true);
          if (!code) {
            await profileCommand("lock_profile", { preserveAuth: false });
            setState((old) => (old ? { ...old, session: null } : old));
            setPhase("locked");
            setMode("choose");
          }
        } else reloadApplication();
      }
    });
  }
  if (!recovery && (phase === "loading" || phase === "closing" || phase === "opening")) {
    return (
      <StartupScreen
        profile={selected}
        error={error || undefined}
        message={
          phase === "closing"
            ? t("profiles.locking", { defaultValue: "Locking Wealthfolio…" })
            : undefined
        }
      >
        {error && (
          <Button
            onClick={() => {
              setError("");
              if (closeFailed) void lock(false, intent.current);
              else refreshRef.current?.();
            }}
          >
            {t("retry")}
          </Button>
        )}
      </StartupScreen>
    );
  }
  if (state?.session && !covered && mode !== "manage") {
    const current = state.profiles.find((p) => p.id === state.session?.profileId);
    return (
      <ProfileContext.Provider
        value={{
          profile: current,
          manageProfile: () => {
            setPasswordManagement(current?.lockEnabled ?? false);
            setRemovePassword(false);
            setName(current?.name ?? "");
            setAvatar(current?.avatarId ?? DEFAULT_PROFILE_AVATAR);
            setSelected(current);
            setPassword("");
            setConfirmPassword("");
            setConfirmationInvalid(false);
            setProof("");
            setMode("manage");
          },
          lockProfile: () => void lock(),
          switchProfile: () => void lock(false, "switch"),
        }}
      >
        {children}
      </ProfileContext.Provider>
    );
  }
  const isProfileEditor = !recovery && (mode === "create" || mode === "manage");
  const isProfilePicker = !recovery && (mode === "choose" || mode === "unlock");
  const formActions = (
    <div className="flex flex-wrap gap-2 pt-2">
      <Button disabled={busy} type="submit">
        {mode === "manage" ? "Save changes" : "Save"}
      </Button>
      <Button
        type="button"
        variant="ghost"
        disabled={busy}
        onClick={() => {
          intent.current = "switch";
          setMode("choose");
        }}
      >
        {isProfileEditor ? "Cancel" : "Back"}
      </Button>
    </div>
  );
  const passwordFields = (
    <>
      {(mode === "recover" ||
        (mode === "manage" && passwordManagement && selected?.lockEnabled)) && (
        <div className="space-y-2">
          <Label htmlFor="profile-proof">
            {mode === "recover" ? "Recovery code" : "Current password or recovery code"}
          </Label>
          <PasswordInput
            id="profile-proof"
            showLabel={
              mode === "recover" ? "Show recovery code" : "Show current password or recovery code"
            }
            hideLabel={
              mode === "recover" ? "Hide recovery code" : "Hide current password or recovery code"
            }
            className="h-11 rounded-full bg-white/30 ps-4 backdrop-blur-md dark:bg-white/5"
            value={proof}
            onChange={(e) => setProof(e.target.value)}
            required={mode === "recover" || removePassword || Boolean(password)}
            autoComplete={mode === "recover" ? "off" : "current-password"}
          />
        </div>
      )}
      {(mode === "recover" || (mode === "manage" && passwordManagement && !removePassword)) && (
        <div className="space-y-2">
          <Label htmlFor="profile-password">New password</Label>
          <PasswordInput
            id="profile-password"
            showLabel="Show new password"
            hideLabel="Hide new password"
            className="h-11 rounded-full bg-white/30 ps-4 backdrop-blur-md dark:bg-white/5"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
              setConfirmationInvalid(false);
              setError("");
            }}
            placeholder={`${MIN_PASSWORD_LENGTH}–${MAX_PASSWORD_LENGTH} characters`}
            autoComplete="new-password"
            required={mode !== "manage" || !selected?.lockEnabled || Boolean(confirmPassword)}
          />
          <Label htmlFor="profile-confirm-password">Re-enter password</Label>
          <PasswordInput
            ref={confirmationInput}
            id="profile-confirm-password"
            showLabel="Show re-entered password"
            hideLabel="Hide re-entered password"
            className="h-11 rounded-full bg-white/30 ps-4 backdrop-blur-md dark:bg-white/5"
            value={confirmPassword}
            onChange={(e) => {
              setConfirmPassword(e.target.value);
              setConfirmationInvalid(false);
            }}
            autoComplete="new-password"
            required={mode !== "manage" || !selected?.lockEnabled || Boolean(password)}
            aria-invalid={confirmationInvalid}
            aria-describedby={confirmationInvalid ? "profile-confirm-password-error" : undefined}
          />
          {confirmationInvalid && (
            <p
              id="profile-confirm-password-error"
              role="alert"
              className="text-destructive text-sm"
            >
              Passwords do not match.
            </p>
          )}
        </div>
      )}
    </>
  );
  return (
    <main
      className={`bg-background text-foreground relative flex min-h-dvh items-center justify-center px-6 py-16 ${isProfilePicker ? "profile-lock-screen pt-24" : isProfileEditor ? "profile-lock-screen profile-settings-screen" : ""}`}
      aria-busy={busy}
    >
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 z-50 h-8" />
      {isProfilePicker && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-10 flex justify-center"
        >
          <div className="group/logo pointer-events-auto shrink-0 [perspective:400px]">
            <img
              src="/logo.png"
              alt=""
              className="size-12 object-contain transition-transform duration-1000 ease-[cubic-bezier(0.22,1,0.36,1)] motion-safe:[transform:rotateY(0deg)] motion-safe:group-hover/logo:[transform:rotateY(180deg)] motion-reduce:transition-none"
            />
          </div>
        </div>
      )}
      <div
        className={`w-full space-y-6 ${isProfileEditor ? "max-w-2xl text-left" : "max-w-xl text-center"}`}
      >
        <h1
          ref={heading}
          tabIndex={-1}
          className={isProfilePicker ? "sr-only" : "text-2xl font-semibold outline-none"}
        >
          {recovery
            ? "Save your recovery code"
            : mode === "choose" || mode === "unlock"
              ? "Who's using Wealthfolio?"
              : mode === "create"
                ? "Create a profile"
                : mode === "manage"
                  ? "Your profile"
                  : "Recover your profile"}
        </h1>
        {error && (
          <p role="alert" className="text-destructive break-words">
            {error}
          </p>
        )}
        {!state || state.starting ? (
          <p>Opening Wealthfolio…</p>
        ) : authPending ? (
          <p>Finish or cancel sign-in to continue. Your portfolio is locked.</p>
        ) : recovery ? (
          <>
            <p>
              Keep this code somewhere safe. It can reset your password. It cannot recover a lost
              database encryption key.
            </p>
            <code className="block select-all break-all rounded-xl border p-4 text-lg">
              {recovery}
            </code>
            <Button
              onClick={() => {
                setRecovery(undefined);
                reloadApplication();
              }}
            >
              I&apos;ve saved my recovery code
            </Button>
          </>
        ) : mode === "choose" || mode === "unlock" ? (
          <>
            <div className="flex flex-wrap justify-center gap-6">
              {state.profiles.map((profile) => (
                <button
                  key={profile.id}
                  disabled={busy}
                  onClick={() => select(profile)}
                  aria-pressed={selected?.id === profile.id}
                  className="profile-lock-option focus-visible:outline-ring flex w-28 flex-col items-center gap-4 rounded-2xl p-2 focus-visible:outline focus-visible:outline-2"
                >
                  <span className="profile-lock-avatar">
                    <ProfileAvatar
                      id={profile.avatarId}
                      className="size-20 rounded-full [&_.profile-abstract-sculpture]:scale-90"
                    />
                  </span>
                  <span className="block w-full break-words text-sm font-medium">
                    {profile.name}
                  </span>
                </button>
              ))}
            </div>
            {mode === "unlock" && selected?.lockEnabled && (
              <form
                key={selected.id}
                onSubmit={submit}
                className="mx-auto max-w-xs space-y-3"
                aria-label={`Unlock ${selected.name}`}
              >
                <Label htmlFor="profile-password" className="sr-only">
                  Password
                </Label>
                <PasswordInput
                  ref={passwordInput}
                  id="profile-password"
                  aria-invalid={passwordInvalid}
                  aria-describedby={passwordInvalid ? "profile-password-error" : undefined}
                  showLabel="Show password"
                  hideLabel="Hide password"
                  value={password}
                  onChange={(event) => {
                    setPassword(event.target.value);
                    setPasswordInvalid(false);
                  }}
                  placeholder="Enter password"
                  className={`h-11 rounded-full border-white/40 bg-white/30 ps-10 text-center backdrop-blur-md dark:border-white/10 dark:bg-white/5 ${passwordInvalid ? "profile-password-invalid" : ""}`}
                  autoComplete="current-password"
                  autoFocus
                  required
                  disabled={busy}
                />
                {passwordInvalid && (
                  <span id="profile-password-error" role="alert" className="sr-only">
                    Incorrect password. Try again.
                  </span>
                )}
                <Button disabled={busy} type="submit" className="w-full">
                  Unlock
                </Button>
                <Button
                  type="button"
                  variant="link"
                  className="text-muted-foreground text-xs"
                  disabled={busy}
                  onClick={() => {
                    setMode("recover");
                    setPassword("");
                    setConfirmPassword("");
                    setConfirmationInvalid(false);
                  }}
                >
                  Forgot password?
                </Button>
              </form>
            )}
            <Button
              variant="ghost"
              size="sm"
              className="profile-lock-add text-muted-foreground text-xs"
              disabled={busy}
              onClick={() => {
                setError("");
                setPassword("");
                setConfirmPassword("");
                setConfirmationInvalid(false);
                setProof("");
                setName("");
                setAvatar(DEFAULT_PROFILE_AVATAR);
                setMode("create");
              }}
            >
              {state?.profiles.length === 0 ? "Create profile" : "Add profile"}
            </Button>
            {state?.pendingDeletions?.map((profile) => (
              <div key={profile.id} className="space-y-2 text-sm">
                <p>Deletion of {profile.name} is incomplete.</p>
                <Button
                  variant="outline"
                  disabled={busy}
                  onClick={() => void deleteProfile(profile.name, "", true, profile)}
                >
                  Retry deletion of {profile.name}
                </Button>
              </div>
            ))}
          </>
        ) : (
          <form
            onSubmit={submit}
            className={`mx-auto space-y-5 text-left ${isProfileEditor ? "max-w-2xl" : "max-w-sm"}`}
          >
            {(mode === "create" || mode === "manage") && (
              <>
                <div className="flex items-start gap-5">
                  <span className="profile-lock-avatar shrink-0">
                    <ProfileAvatar
                      id={avatar}
                      className="size-24 rounded-full [&_.profile-abstract-sculpture]:scale-90"
                    />
                  </span>
                  <div className="min-w-0 max-w-xs flex-1 space-y-2">
                    <Label htmlFor="profile-name">Name</Label>
                    <Input
                      id="profile-name"
                      className="h-11 rounded-full bg-white/30 px-4 backdrop-blur-md dark:bg-white/5"
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      maxLength={60}
                      required
                      autoFocus
                    />
                    {mode === "manage" && (!passwordManagement || removePassword) && (
                      <Button
                        type="button"
                        variant="link"
                        className="text-muted-foreground h-auto px-0 py-2 text-sm"
                        disabled={busy}
                        onClick={() => {
                          setPasswordManagement(true);
                          setRemovePassword(false);
                        }}
                      >
                        Enable password
                      </Button>
                    )}
                    {mode === "manage" && passwordManagement && (
                      <div className="space-y-4 pt-2">
                        {passwordFields}
                        {!removePassword && (
                          <Button
                            type="button"
                            variant="link"
                            className="text-muted-foreground h-auto px-0 py-0 text-sm"
                            disabled={busy}
                            onClick={() => {
                              setPassword("");
                              setConfirmPassword("");
                              setConfirmationInvalid(false);
                              if (selected?.lockEnabled) {
                                setRemovePassword(true);
                              } else {
                                setPasswordManagement(false);
                                setProof("");
                              }
                            }}
                          >
                            Disable password
                          </Button>
                        )}
                      </div>
                    )}
                    {formActions}
                  </div>
                </div>
              </>
            )}
            {mode === "recover" && passwordFields}
            {isProfileEditor && <ProfileAvatarPicker value={avatar} onChange={setAvatar} />}
            {!isProfileEditor && formActions}
            {mode === "manage" && (
              <section className="border-destructive/25 space-y-2 border-t pt-6">
                <h2 className="font-medium">Delete profile</h2>
                <p className="text-muted-foreground text-sm">
                  Permanently remove this profile and its local data.
                </p>
                <Button
                  type="button"
                  variant="destructive"
                  disabled={busy}
                  onClick={() => setDeleteOpen(true)}
                >
                  Delete profile
                </Button>
              </section>
            )}
          </form>
        )}
        {deleteOpen && selected && (
          <DeleteProfileDialog
            error={error}
            profile={selected}
            onClose={() => setDeleteOpen(false)}
            onDelete={(confirmation, proof) => void deleteProfile(confirmation, proof)}
          />
        )}
      </div>
    </main>
  );
}
