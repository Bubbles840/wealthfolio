import { isWeb } from "@/adapters";
import { profileScope } from "./session";
import type { ProfileSession } from "./session";
export interface ProfileSummary {
  id: string;
  name: string;
  avatarId: string;
  lockEnabled: boolean;
}
export interface ProfileState {
  profiles: ProfileSummary[];
  pendingDeletions?: ProfileSummary[];
  session: ProfileSession | null;
  starting: boolean;
}
export async function profileCommand<T>(
  command: string,
  payload: Record<string, unknown> = {},
  scoped = false,
): Promise<T> {
  if (!isWeb) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, { ...payload, ...(scoped ? { scopeId: profileScope() } : {}) });
  }
  const res = await fetch(`/api/v1/profiles/${command}`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...(scoped ? { "x-wf-profile-scope": profileScope() } : {}),
    },
    body: JSON.stringify(payload),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json() as Promise<T>;
}
