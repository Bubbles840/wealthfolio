import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useNativePrivacyCover } from "./use-native-privacy-cover";
const mocks = vi.hoisted(() => ({ command: vi.fn(), epoch: 7 }));
vi.mock("@/adapters", () => ({ isWeb: false, logger: { error: vi.fn() } }));
vi.mock("./api", () => ({ profileCommand: mocks.command }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
let frames: Map<number, FrameRequestCallback>;
let sequence: number;
beforeEach(() => {
  vi.useFakeTimers();
  frames = new Map();
  sequence = 0;
  mocks.epoch = 7;
  mocks.command
    .mockReset()
    .mockImplementation(async (command) =>
      command === "profile_cover_state" ? mocks.epoch : undefined,
    );
  vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => {
    frames.set(++sequence, fn);
    return sequence;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
async function paint() {
  await act(async () => {
    const current = [...frames.values()];
    frames.clear();
    current.forEach((fn) => fn(0));
  });
}
it("keeps privacy protection until a safe frame and its paint handoff", async () => {
  const hook = renderHook(({ safe }) => useNativePrivacyCover(safe), {
    initialProps: { safe: false },
  });
  await act(async () => {});
  expect(frames.size).toBe(0);
  hook.rerender({ safe: true });
  await paint();
  expect(mocks.command).not.toHaveBeenCalledWith("profile_cover_ready", expect.anything());
  await paint();
  expect(mocks.command).toHaveBeenCalledWith("profile_cover_ready", { epoch: 7 });
});
it("cancels an old acknowledgement when a newer lock arrives", async () => {
  renderHook(() => useNativePrivacyCover(true));
  await act(async () => {});
  await paint();
  mocks.epoch = 8;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  await paint();
  await paint();
  expect(mocks.command).not.toHaveBeenCalledWith("profile_cover_ready", { epoch: 7 });
  expect(mocks.command).toHaveBeenCalledWith("profile_cover_ready", { epoch: 8 });
});
it("retries an acknowledgement failure without requiring a profile-state change", async () => {
  let attempts = 0;
  mocks.command.mockImplementation(async (command) => {
    if (command === "profile_cover_state") return 7;
    if (++attempts === 1) throw new Error("IPC temporarily unavailable");
  });
  renderHook(() => useNativePrivacyCover(true));
  await act(async () => {});
  await paint();
  await paint();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  await paint();
  await paint();
  expect(attempts).toBe(2);
});
