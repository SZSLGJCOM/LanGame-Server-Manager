import type { TranslateFn } from "../../i18n";
import { formatDesktopError } from "../../desktop-error-message";
import type { SteamWorkshopLookupItem } from "../../types";

export interface WorkshopWarning {
  summary: string;
  details: string;
  includesOriginalText: boolean;
}

const WORKSHOP_ORIGINS = new Set([
  "https://steamcommunity.com", "https://steamcommunity-a.akamaihd.net",
  "https://api.steampowered.com", "https://api.steamchina.com"
]);

function isWorkshopRateLimit(error: string | null | undefined): boolean {
  if (!error) return false;
  try {
    const value: unknown = JSON.parse(error);
    if (!value || typeof value !== "object" || Array.isArray(value)) return false;
    const entry = value as Record<string, unknown>;
    return entry.code === "steam_workshop_network_failed" && entry.reason === "http" && entry.status === 429
      && typeof entry.origin === "string" && WORKSHOP_ORIGINS.has(entry.origin);
  } catch { return false; }
}

function diagnostics(t: TranslateFn, error: string): string {
  const readable = formatDesktopError(t, error);
  return readable === error ? error : `${readable}\n${error}`;
}

/** Keep the visible state stable while item counts and Steam's cooldown diagnostics change. */
export function buildWorkshopVerificationWarning(
  t: TranslateFn,
  items: SteamWorkshopLookupItem[],
  selectedItem?: SteamWorkshopLookupItem | null
): WorkshopWarning | null {
  const unverified = items.filter((item) => item.status === "unverified");
  if (!unverified.length) return null;
  const localizationWarning = selectedItem?.localization_warning;
  const includesOriginalText = Boolean(localizationWarning && unverified.some((item) =>
    item.message === localizationWarning || (isWorkshopRateLimit(item.message) && isWorkshopRateLimit(localizationWarning))));
  const summary = t(unverified.some((item) => isWorkshopRateLimit(item.message))
    ? "servers.mods.warning.rateLimited" : "servers.mods.warning.unverified");
  const evidence = new Map<string, string[]>();
  for (const item of unverified) {
    const error = item.message || t("servers.mods.workshopUnavailable");
    evidence.set(error, [...(evidence.get(error) ?? []), item.id]);
  }
  const details = [...evidence].map(([error, ids]) => `${ids.join(", ")}\n${diagnostics(t, error)}`);
  if (includesOriginalText && localizationWarning && selectedItem && !evidence.has(localizationWarning)) {
    details.push(`${selectedItem.id}\n${diagnostics(t, localizationWarning)}`);
  }
  return {
    summary: includesOriginalText ? `${summary} ${t("servers.mods.warning.originalText")}` : summary,
    details: details.join("\n\n"),
    includesOriginalText
  };
}

export function buildWorkshopLocalizationWarning(t: TranslateFn, error: string): WorkshopWarning {
  return {
    summary: t(isWorkshopRateLimit(error)
      ? "servers.mods.warning.localizedRateLimited" : "servers.mods.storeDetail.originalTextFallback"),
    details: diagnostics(t, error),
    includesOriginalText: true
  };
}
