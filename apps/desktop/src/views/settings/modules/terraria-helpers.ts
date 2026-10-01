import type { TranslateFn } from "../../../i18n";

export const TERRARIA_JOURNEY_PERMISSION_KEYS = [
  "journeypermission_time_setfrozen",
  "journeypermission_time_setdawn",
  "journeypermission_time_setnoon",
  "journeypermission_time_setdusk",
  "journeypermission_time_setmidnight",
  "journeypermission_time_setspeed",
  "journeypermission_godmode",
  "journeypermission_setdifficulty",
  "journeypermission_setspawnrate",
  "journeypermission_increaseplacementrange",
  "journeypermission_biomespread_setfrozen",
  "journeypermission_wind_setstrength",
  "journeypermission_wind_setfrozen",
  "journeypermission_rain_setstrength",
  "journeypermission_rain_setfrozen"
] as const;

const TERRARIA_LANGUAGE_LABEL_KEYS: Record<string, string> = {
  "en-US": "settings.schema.terraria.language.option.en_us",
  "de-DE": "settings.schema.terraria.language.option.de_de",
  "it-IT": "settings.schema.terraria.language.option.it_it",
  "fr-FR": "settings.schema.terraria.language.option.fr_fr",
  "es-ES": "settings.schema.terraria.language.option.es_es",
  "ru-RU": "settings.schema.terraria.language.option.ru_ru",
  "zh-Hans": "settings.schema.terraria.language.option.zh_hans",
  "pt-BR": "settings.schema.terraria.language.option.pt_br",
  "pl-PL": "settings.schema.terraria.language.option.pl_pl",
  "ja-JP": "settings.schema.terraria.language.option.ja_jp",
  "ko-KR": "settings.schema.terraria.language.option.ko_kr",
  "zh-Hant": "settings.schema.terraria.language.option.zh_hant"
};

export interface TerrariaJourneyPermissionSummary {
  locked: number;
  hostOnly: number;
  everyone: number;
  total: number;
}

export function terrariaReadString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function terrariaReadBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    return !["", "0", "false", "off", "no"].includes(value.trim().toLowerCase());
  }
  return Boolean(value);
}

export function terrariaReadNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function terrariaParseSimpleLines(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

export function terrariaCountSimpleLines(value: unknown): number {
  return terrariaParseSimpleLines(value).length;
}

export function formatTerrariaWorldSize(value: unknown, t: TranslateFn): string {
  switch (terrariaReadNumber(value)) {
    case 1:
      return t("terraria.settings.enum.worldSize.small", undefined, "Small");
    case 2:
      return t("terraria.settings.enum.worldSize.medium", undefined, "Medium");
    case 3:
      return t("terraria.settings.enum.worldSize.large", undefined, "Large");
    default:
      return t("common.waiting", undefined, "--");
  }
}

export function formatTerrariaDifficulty(value: unknown, t: TranslateFn): string {
  switch (terrariaReadNumber(value)) {
    case 0:
      return t("terraria.settings.enum.difficulty.classic", undefined, "Classic");
    case 1:
      return t("terraria.settings.enum.difficulty.expert", undefined, "Expert");
    case 2:
      return t("terraria.settings.enum.difficulty.master", undefined, "Master");
    case 3:
      return t("terraria.settings.enum.difficulty.journey", undefined, "Journey");
    default:
      return t("common.waiting", undefined, "--");
  }
}

export function formatTerrariaSpecialSeed(value: unknown, t: TranslateFn): string {
  switch (terrariaReadString(value).trim().toLowerCase()) {
    case "celebration":
      return "Celebrationmk10";
    case "theconstant":
      return "The Constant";
    case "notthebees":
      return "Not the Bees";
    case "notraps":
      return "No Traps";
    case "fortheworthy":
      return "For the Worthy";
    case "remix":
      return "Remix";
    case "drunk":
      return "Drunk";
    case "zenith":
      return "Zenith";
    default:
      return t("terraria.settings.summary.seedStandard", undefined, "Standard");
  }
}

export function formatTerrariaSteamLobby(
  settings: Record<string, unknown>,
  t: TranslateFn
): string {
  if (!terrariaReadBoolean(settings.steam)) {
    return t("terraria.settings.summary.lobbyDisabled", undefined, "Disabled");
  }

  switch (terrariaReadString(settings.lobby).trim().toLowerCase()) {
    case "friends":
      return t("terraria.settings.summary.lobbyFriends", undefined, "Friends Only");
    case "private":
      return t("terraria.settings.summary.lobbyPrivate", undefined, "Private");
    default:
      return t("terraria.settings.summary.lobbySteamEnabled", undefined, "Steam Enabled");
  }
}

export function formatTerrariaPriority(value: unknown, t: TranslateFn): string {
  switch (terrariaReadNumber(value)) {
    case 0:
      return t("terraria.settings.enum.priority.realtime", undefined, "Realtime");
    case 1:
      return t("terraria.settings.enum.priority.high", undefined, "High");
    case 2:
      return t("terraria.settings.enum.priority.aboveNormal", undefined, "Above Normal");
    case 3:
      return t("terraria.settings.enum.priority.normal", undefined, "Normal");
    case 4:
      return t("terraria.settings.enum.priority.belowNormal", undefined, "Below Normal");
    case 5:
      return t("terraria.settings.enum.priority.idle", undefined, "Idle");
    default:
      return t("common.waiting", undefined, "--");
  }
}

export function formatTerrariaLanguage(value: unknown, t: TranslateFn): string {
  const normalizedValue = terrariaReadString(value).trim();
  if (normalizedValue.length === 0) {
    return "";
  }

  const key = TERRARIA_LANGUAGE_LABEL_KEYS[normalizedValue];
  if (!key) {
    return normalizedValue;
  }

  return t(key, undefined, normalizedValue);
}

export function formatTerrariaAnnouncementRange(value: unknown, t: TranslateFn): string {
  const parsed = terrariaReadNumber(value);
  if (parsed === null) {
    return t("common.waiting", undefined, "--");
  }

  if (parsed === -1) {
    return t("terraria.settings.summary.announcementRangeServer", undefined, "Server-wide");
  }

  return `${parsed} px`;
}

export function formatTerrariaJourneyPermissionLevel(
  value: unknown,
  t: TranslateFn
): string {
  switch (terrariaReadNumber(value)) {
    case 0:
      return t("terraria.settings.enum.permission.locked", undefined, "Locked");
    case 1:
      return t("terraria.settings.enum.permission.hostOnly", undefined, "Host Only");
    case 2:
      return t("terraria.settings.enum.permission.everyone", undefined, "Everyone");
    default:
      return t("common.waiting", undefined, "--");
  }
}

export function formatTerrariaPasswordState(
  value: unknown,
  t: TranslateFn
): string {
  return terrariaReadString(value).trim().length > 0
    ? t("terraria.settings.summary.passwordSet", undefined, "Set")
    : t("terraria.settings.summary.passwordOpen", undefined, "Open");
}

export function summarizeTerrariaJourneyPermissions(
  settings: Record<string, unknown>
): TerrariaJourneyPermissionSummary {
  const summary: TerrariaJourneyPermissionSummary = {
    locked: 0,
    hostOnly: 0,
    everyone: 0,
    total: TERRARIA_JOURNEY_PERMISSION_KEYS.length
  };

  for (const key of TERRARIA_JOURNEY_PERMISSION_KEYS) {
    switch (terrariaReadNumber(settings[key])) {
      case 0:
        summary.locked += 1;
        break;
      case 1:
        summary.hostOnly += 1;
        break;
      case 2:
      default:
        summary.everyone += 1;
        break;
    }
  }

  return summary;
}

export function formatTerrariaJourneySummary(
  settings: Record<string, unknown>,
  t: TranslateFn
): string {
  const values = TERRARIA_JOURNEY_PERMISSION_KEYS
    .map((key) => terrariaReadNumber(settings[key]))
    .filter((value): value is number => value !== null);

  if (values.length === 0) {
    return t("terraria.settings.summary.permissionDefault", undefined, "Everyone");
  }

  const firstValue = values[0];
  return values.every((value) => value === firstValue)
    ? formatTerrariaJourneyPermissionLevel(firstValue, t)
    : t("terraria.settings.summary.permissionMixed", undefined, "Mixed");
}
