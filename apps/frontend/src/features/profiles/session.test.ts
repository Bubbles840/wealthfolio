import { beforeEach, describe, expect, it, vi } from "vitest";
describe("profile request authority", () => {
  beforeEach(() => vi.resetModules());
  it("does not allow an old async operation to acquire a new scope", async () => {
    const session = await import("./session");
    expect(() => session.profileScope()).toThrow("PROFILE_LOCKED");
    session.installProfileSession({ profileId: "a", scopeId: "scope-a" });
    expect(session.profileScope()).toBe("scope-a");
    session.revokeProfileSession();
    expect(() => session.profileScope()).toThrow("PROFILE_LOCKED");
  });
  it("discards a response that arrives after lock", async () => {
    const session = await import("./session");
    session.installProfileSession({ profileId: "a", scopeId: "scope-a" });
    let release!: (response: Response) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            release = resolve;
          }),
      ),
    );
    const response = session.profileFetch("/api/v1/accounts");
    session.revokeProfileSession();
    release(new Response("private"));
    await expect(response).rejects.toThrow("PROFILE_LOCKED");
    vi.unstubAllGlobals();
  });
});

it("does not reload for a stale status response during pending native auth", async () => {
  vi.resetModules();
  const session = await import("./session");
  session.installProfileSession({ profileId: "a", scopeId: "scope-a" });
  session.deferProfileReload(true);
  session.revokeProfileSession();
  expect(session.installProfileSession({ profileId: "a", scopeId: "scope-a" })).toBe(false);
  expect(() => session.profileScope()).toThrow("PROFILE_LOCKED");
});

it("reloads for a fresh recovery grant even after revocation, without admitting it in the old document", async () => {
  vi.resetModules();
  const reload = vi.fn();
  vi.doMock("@/lib/reload-application", () => ({
    reloadApplication: reload,
    deferApplicationReload: vi.fn(),
  }));
  const session = await import("./session");
  session.installProfileSession({ profileId: "a", scopeId: "old" });
  session.revokeProfileSession();
  expect(session.installProfileSession({ profileId: "a", scopeId: "new" })).toBe(false);
  expect(reload).toHaveBeenCalledWith({ dashboard: false });
  expect(() => session.profileScope()).toThrow("PROFILE_LOCKED");
  vi.doUnmock("@/lib/reload-application");
});
it("routes cross-tab profile changes to the dashboard", async () => {
  vi.resetModules();
  const reload = vi.fn();
  vi.doMock("@/lib/reload-application", () => ({
    reloadApplication: reload,
    deferApplicationReload: vi.fn(),
  }));
  const session = await import("./session");
  session.installProfileSession({ profileId: "a", scopeId: "old" });
  expect(session.installProfileSession({ profileId: "b", scopeId: "new" })).toBe(false);
  expect(reload).toHaveBeenCalledWith({ dashboard: true });
  expect(() => session.profileScope()).toThrow("PROFILE_LOCKED");
  vi.doUnmock("@/lib/reload-application");
});
