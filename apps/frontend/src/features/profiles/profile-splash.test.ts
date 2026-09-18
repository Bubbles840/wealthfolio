import html from "../../../index.html?raw";
import { afterEach, expect, it, vi } from "vitest";

const bootstrap = html.match(/<script>([\s\S]*?)<\/script>/)![1];

afterEach(() => {
  sessionStorage.clear();
  document.documentElement.classList.remove("profile-opening");
  vi.restoreAllMocks();
});

it("uses the generic splash only for a recent profile-opening hint", () => {
  sessionStorage.setItem("wealthfolio-profile-opening", JSON.stringify({ at: Date.now() }));
  new Function(bootstrap)();
  expect(document.documentElement).toHaveClass("profile-opening");
});

it.each([
  null,
  "invalid json",
  JSON.stringify({ at: 0 }),
  JSON.stringify({ at: Date.now() + 60000 }),
])("keeps the normal splash for an absent, malformed, expired, or future hint: %s", (hint) => {
  if (hint !== null) sessionStorage.setItem("wealthfolio-profile-opening", hint);
  new Function(bootstrap)();
  expect(document.documentElement).not.toHaveClass("profile-opening");
});

it("can still boot when presentation storage is unavailable", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("Unavailable");
  });
  expect(() => new Function(bootstrap)()).not.toThrow();
  expect(document.documentElement).not.toHaveClass("profile-opening");
});
