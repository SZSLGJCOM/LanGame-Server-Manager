import type { TranslateFn } from "../../../i18n";
import type { GuidedSettingsValidationIssue, SettingsObject } from "../settings-schema";
import {
  applyVRisingTypedGameSettingsFromRawJson,
  findVRisingInvalidNativeValue,
  syncVRisingServerGameSettingsFromTypedFields,
  VRISING_TYPED_GAME_SETTING_KEYS
} from "./vrising-game-settings";

export function getVRisingRawSettingsIssue(
  settings: Readonly<SettingsObject>,
  t: TranslateFn
): GuidedSettingsValidationIssue | null {
  const issue = (reason: string, message: string): GuidedSettingsValidationIssue => ({
    fieldKey: "server_game_settings_json", reason, message
  });
  const raw = settings.server_game_settings_json;
  if (typeof raw !== "string" || !raw.trim()) {
    return issue("empty", t(
      "settings.vrising.serverGameSettingsEmpty",
      undefined,
      "ServerGameSettings.json overrides cannot be empty."
    ));
  }
  try {
    const parsed = JSON.parse(raw);
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
      return issue("object", t(
        "settings.vrising.serverGameSettingsObject",
        undefined,
        "ServerGameSettings.json overrides must be a JSON object."
      ));
    }
    const invalidPath = findVRisingInvalidNativeValue(parsed);
    if (invalidPath) {
      return issue("nativeValue", t(
        "settings.configuration.validation.pattern",
        { field: invalidPath },
        `${invalidPath} does not match the required format.`
      ));
    }
  } catch (error) {
    return issue("json", t(
      "settings.vrising.serverGameSettingsInvalid",
      { message: String((error as Error).message || error) },
      "ServerGameSettings.json is not valid JSON: {message}"
    ));
  }
  return null;
}

export function initializeVRisingSettings(
  settings: Readonly<SettingsObject>,
  t: TranslateFn
): SettingsObject {
  const hasRaw = Object.prototype.hasOwnProperty.call(settings, "server_game_settings_json");
  if (hasRaw && getVRisingRawSettingsIssue(settings, t)) return { ...settings };
  return syncVRisingServerGameSettingsFromTypedFields(
    applyVRisingTypedGameSettingsFromRawJson({ ...settings })
  );
}

export function applyVRisingSettingsPatch(
  settings: Readonly<SettingsObject>,
  patch: Readonly<SettingsObject>,
  t: TranslateFn
): SettingsObject {
  const changedKeys = Object.keys(patch);
  let next: SettingsObject = { ...settings, ...patch };
  const hasRaw = Object.prototype.hasOwnProperty.call(next, "server_game_settings_json");
  if (hasRaw && getVRisingRawSettingsIssue(next, t)) return next;
  if (changedKeys.includes("server_game_settings_json")) {
    next = applyVRisingTypedGameSettingsFromRawJson(next);
  }
  return changedKeys.some((key) => VRISING_TYPED_GAME_SETTING_KEYS.has(key))
    ? syncVRisingServerGameSettingsFromTypedFields(next)
    : next;
}
