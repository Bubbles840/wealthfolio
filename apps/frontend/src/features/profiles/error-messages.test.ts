import { createInstance } from "i18next";
import { beforeAll, describe, expect, it } from "vitest";
import en from "@/i18n/locales/en/common.json";
import { profileErrorMessage } from "./error-messages";

const i18n = createInstance();
beforeAll(async () => {
  await i18n.init({ lng: "en", fallbackLng: false, resources: { en: { common: en } } });
});

it.each([
  ["PROFILE_LOCKED", "Enter your profile password to continue."],
  ["PROFILE_STALE", "Your session has ended. Unlock your profile again."],
  ["PROFILE_NOT_FOUND", "This profile is no longer available. Choose another profile."],
  ["PROFILE_PASSWORD_INVALID", "The password or recovery code is incorrect. Please try again."],
  ["PROFILE_COOLDOWN: Try again in 1 seconds.", "Too many attempts. Try again in 1 second."],
  ['"PROFILE_COOLDOWN: Try again in 30 seconds."', "Too many attempts. Try again in 30 seconds."],
  ["PROFILE_COOLDOWN", "Too many attempts. Wait a moment before trying again."],
  [
    "PROFILE_INVALID: Use a profile name of 1–60 characters.",
    "Use a profile name of 1–60 characters.",
  ],
  [
    "PROFILE_INVALID: Enter the profile name to confirm deletion.",
    "Enter the profile name exactly to confirm deletion.",
  ],
  [
    "PROFILE_UNAVAILABLE: /private/secret/database",
    "We couldn’t open this profile. Restart Wealthfolio and try again.",
  ],
  ["database /private/secret/database", "Something went wrong. Please try again."],
])("renders safe copy for %s", (diagnostic, message) => {
  expect(profileErrorMessage(new Error(diagnostic), i18n.getFixedT("en", "common"))).toBe(message);
});

const catalogs = import.meta.glob<{ default: typeof en }>("../../i18n/locales/*/common.json", {
  eager: true,
});
interface Messages {
  [key: string]: string | Messages;
}
function flatten(value: Messages, prefix = ""): Record<string, string> {
  return Object.fromEntries(
    Object.entries(value).flatMap(([key, text]) =>
      typeof text === "string"
        ? [[`${prefix}${key}`, text]]
        : Object.entries(flatten(text, `${prefix}${key}.`)),
    ),
  );
}
const english = flatten(en.profiles);
const placeholders = (value: string) =>
  [...value.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1]).sort();

describe.each(Object.entries(catalogs))("profile translations: %s", (path, catalog) => {
  it("contains every message and preserves interpolation variables", () => {
    const translated = flatten(catalog.default.profiles);
    for (const [key, copy] of Object.entries(english)) {
      expect(translated[key], key).toBeTruthy();
      expect(placeholders(translated[key]), key).toEqual(placeholders(copy));
    }
  });
  it("renders cooldowns in the selected language without falling back to English", async () => {
    const lang = path.split("/").at(-2)!;
    const instance = createInstance();
    await instance.init({
      lng: lang,
      fallbackLng: false,
      resources: { [lang]: { common: catalog.default } },
    });
    const t = instance.getFixedT(lang, "common");
    expect(profileErrorMessage("PROFILE_COOLDOWN: Try again in 30 seconds.", t)).toBe(
      catalog.default.profiles.errors.cooldown_other.replace("{{count}}", "30"),
    );
    expect(profileErrorMessage("CONNECT_REBIND_REQUIRED: internal details", t)).toBe(
      catalog.default.profiles.errors.rebind,
    );
  });
});
