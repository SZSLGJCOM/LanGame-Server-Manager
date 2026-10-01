import type { TranslateFn } from "../../../i18n";
import type { SettingsObject } from "../settings-schema";

const OPTIONAL_TEXT_FIELDS = new Set([
  "native_browser_thumbnail", "native_browser_desc_server_list", "native_browser_monetization", "browser_links_json"
]);

export function applyUnturnedSettingsPatch(settings: Readonly<SettingsObject>, patch: Readonly<SettingsObject>): SettingsObject {
  const next = { ...settings, ...patch };
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined || (OPTIONAL_TEXT_FIELDS.has(key) && (value === null || value === ""))) delete next[key];
  }
  return next;
}

export function validateUnturnedLobbyLinks(value: unknown, t: TranslateFn): string | undefined {
  if (value === undefined || value === null || value === "") return undefined;
  const invalid = () => t("unturned.settings.validation.lobbyLinks", undefined,
    "Enter up to 32 links as a JSON array, each containing only Message and an HTTP or HTTPS URL.");
  const byteLength = (text: string) => new TextEncoder().encode(text).length;
  if (typeof value !== "string" || byteLength(value) > 65536) return invalid();
  let parsed: unknown;
  try { parsed = JSON.parse(value); } catch { return invalid(); }
  if (!Array.isArray(parsed) || parsed.length > 32) return invalid();
  for (const entry of parsed) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return invalid();
    const link = entry as Record<string, unknown>;
    if (Object.keys(link).length !== 2 || typeof link.Message !== "string" || !link.Message
      || byteLength(link.Message) > 1024 || link.Message.includes("\0") || typeof link.URL !== "string"
      || byteLength(link.URL) > 2048 || !/^https?:\/\//.test(link.URL) || /[\s\p{Cc}]/u.test(link.URL)) return invalid();
  }
  return undefined;
}
