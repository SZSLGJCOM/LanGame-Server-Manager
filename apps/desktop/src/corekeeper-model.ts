import type { TranslateFn } from "./i18n";
import type { InstanceDetails } from "./types";

export interface CoreKeeperSteamIdListParseResult {
  entries: string[];
  invalidEntries: string[];
}

function readString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function readBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["true", "1", "yes", "on"].includes(normalized)) {
      return true;
    }
    if (["false", "0", "no", "off", ""].includes(normalized)) {
      return false;
    }
  }
  return Boolean(value);
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function parseCoreKeeperSettingsJson(settingsJson: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(settingsJson) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

export function normalizeCoreKeeperGameId(raw: unknown): string | null {
  const sanitized = readString(raw)
    .split("")
    .filter((character) => /[a-z0-9]/i.test(character))
    .join("");

  return sanitized.length >= 15 && sanitized.length <= 28 ? sanitized : null;
}

export function deriveCoreKeeperGameId(instanceId: string): string {
  const sanitizedInstanceId = readString(instanceId)
    .split("")
    .filter((character) => /[a-z0-9]/i.test(character))
    .join("")
    .toLowerCase();

  let derived = `lgm${sanitizedInstanceId}corekeeper`;
  if (derived.length < 15) {
    derived += "server";
  }
  if (derived.length < 15) {
    derived += "0".repeat(15 - derived.length);
  }
  return derived.slice(0, 28);
}

export function resolveCoreKeeperEffectiveGameId(
  settings: Record<string, unknown>,
  instanceId: string
): string {
  return normalizeCoreKeeperGameId(settings.game_id) ?? deriveCoreKeeperGameId(instanceId);
}

export function parseCoreKeeperSteamIdList(raw: unknown): CoreKeeperSteamIdListParseResult {
  const seenValid = new Set<string>();
  const seenInvalid = new Set<string>();
  const entries: string[] = [];
  const invalidEntries: string[] = [];
  const normalized = readString(raw).replace(/\r\n/g, "\n").replace(/\r/g, "\n");

  for (const value of normalized
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter(Boolean)) {
    if (value.startsWith("#") || value.startsWith("//")) {
      continue;
    }

    if (/^\d{17}$/.test(value)) {
      if (!seenValid.has(value)) {
        seenValid.add(value);
        entries.push(value);
      }
      continue;
    }

    if (!seenInvalid.has(value)) {
      seenInvalid.add(value);
      invalidEntries.push(value);
    }
  }

  return {
    entries,
    invalidEntries
  };
}

export function normalizeCoreKeeperSteamIdList(raw: unknown): string[] {
  return parseCoreKeeperSteamIdList(raw).entries;
}

export function readCoreKeeperPort(details: InstanceDetails, portName = "game"): number | null {
  return details.ports.find((port) => port.name === portName)?.port ?? null;
}

export function isCoreKeeperDirectConnectionEnabled(settings: Record<string, unknown>): boolean {
  return readBoolean(settings.direct_connection_enabled);
}

export function resolveCoreKeeperJoinPassword(settings: Record<string, unknown>): string {
  return readString(settings.join_password).trim();
}

export function resolveCoreKeeperAllowedPlatformCode(settings: Record<string, unknown>): number {
  const parsed = readNumber(settings.allowed_platform_code);
  return parsed !== null ? parsed : 0;
}

export function formatCoreKeeperWorldMode(value: unknown, t: TranslateFn): string {
  switch (Number(value)) {
    case 1:
      return t("corekeeper.settings.worldMode.hard", undefined, "Hard");
    case 2:
      return t("corekeeper.settings.worldMode.creative", undefined, "Creative");
    case 4:
      return t("corekeeper.settings.worldMode.casual", undefined, "Casual");
    case 0:
    default:
      return t("corekeeper.settings.worldMode.standard", undefined, "Standard");
  }
}

export function formatCoreKeeperSeasonOverride(value: unknown, t: TranslateFn): string {
  switch (Number(value)) {
    case 0:
      return t("corekeeper.settings.season.disabled", undefined, "Disabled");
    case 1:
      return t("corekeeper.settings.season.easter", undefined, "Easter");
    case 2:
      return t("corekeeper.settings.season.halloween", undefined, "Halloween");
    case 3:
      return t("corekeeper.settings.season.christmas", undefined, "Christmas");
    case 4:
      return t("corekeeper.settings.season.valentines", undefined, "Valentine's");
    case 5:
      return t("corekeeper.settings.season.anniversary", undefined, "Anniversary");
    case 6:
      return t("corekeeper.settings.season.cherryBlossom", undefined, "Cherry Blossom Festival");
    case 7:
      return t("corekeeper.settings.season.lunarNewYear", undefined, "Lunar New Year");
    case -1:
    default:
      return t("corekeeper.settings.season.automatic", undefined, "Automatic");
  }
}

export function formatCoreKeeperAllowedPlatform(value: unknown, t: TranslateFn): string {
  switch (Number(value)) {
    case 1:
      return t("corekeeper.settings.platform.steam", undefined, "Steam only");
    case 2:
      return t("corekeeper.settings.platform.epic", undefined, "Epic only");
    case 3:
      return t("corekeeper.settings.platform.microsoft", undefined, "Microsoft Store only");
    case 4:
      return t("corekeeper.settings.platform.gog", undefined, "GOG only");
    case 0:
    default:
      return t("corekeeper.settings.platform.all", undefined, "All PC platforms");
  }
}

export function formatCoreKeeperJoinMode(directConnectionEnabled: boolean, t: TranslateFn): string {
  return directConnectionEnabled
    ? t("servers.corekeeper.valueDirectMode", undefined, "Direct IP")
    : t("servers.corekeeper.valueRelayMode", undefined, "Steam relay");
}

export function formatCoreKeeperSecretState(
  value: string,
  t: TranslateFn,
  options?: {
    placeholder?: string;
    openWhenEmpty?: boolean;
  }
): string {
  const trimmed = readString(value).trim();
  if (!trimmed) {
    return options?.openWhenEmpty
      ? t("servers.corekeeper.valueOpen", undefined, "Open")
      : t("servers.corekeeper.valueNotSet", undefined, "Not set");
  }
  if (options?.placeholder && trimmed === options.placeholder) {
    return t("servers.corekeeper.valuePlaceholder", undefined, "Placeholder");
  }
  return t("servers.corekeeper.valueConfigured", undefined, "Configured");
}
