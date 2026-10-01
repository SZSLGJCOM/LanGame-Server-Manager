import type { TranslateFn } from "../../../i18n";
import type { GuidedFieldCopy } from "../settings-schema";
import { EN_US_CONAN_EXILES_MESSAGES } from "../../../i18n/games/conanexiles.en";
import { ZH_CN_CONAN_EXILES_MESSAGES } from "../../../i18n/games/conanexiles.zh-cn";

const FIELD_COPY: Record<
  string,
  {
    titleKey: string;
    fallbackTitle: string;
    descriptionKey?: string;
    fallbackDescription?: string;
  }
> = {
  server_name: {
    titleKey: "settings.schema.conanexiles.server_name.title",
    fallbackTitle: "Server Name",
    descriptionKey: "settings.schema.conanexiles.server_name.description",
    fallbackDescription: "Shown in the Conan Exiles browser and in the join text you share with players."
  },
  max_players: {
    titleKey: "settings.schema.conanexiles.max_players.title",
    fallbackTitle: "Max Players",
    descriptionKey: "settings.schema.conanexiles.max_players.description",
    fallbackDescription: "Player slot cap passed into the dedicated server launch plan."
  },
  excluded_regions: {
    titleKey: "settings.schema.conanexiles.excluded_regions.title",
    fallbackTitle: "Loaded Regions",
    descriptionKey: "settings.schema.conanexiles.excluded_regions.description",
    fallbackDescription: "Load both Enhanced regions, or disable Isle of Siptah to reduce server RAM use."
  },
  server_password: {
    titleKey: "settings.schema.conanexiles.server_password.title",
    fallbackTitle: "Join Password",
    descriptionKey: "settings.schema.conanexiles.server_password.description",
    fallbackDescription: "Written into Engine.ini as the room password. Leave empty only when the server should stay open."
  },
  admin_password: {
    titleKey: "settings.schema.conanexiles.admin_password.title",
    fallbackTitle: "Admin Password",
    descriptionKey: "settings.schema.conanexiles.admin_password.description",
    fallbackDescription: "Highest-trust operator credential for in-game admin access. Replace the placeholder before daily use."
  },
  rcon_enabled: {
    titleKey: "settings.schema.conanexiles.rcon_enabled.title",
    fallbackTitle: "Enable RCON",
    descriptionKey: "settings.schema.conanexiles.rcon_enabled.description",
    fallbackDescription: "Turns on Conan Exiles remote console so host tools can reach the instance over the reserved RCON port."
  },
  rcon_password: {
    titleKey: "settings.schema.conanexiles.rcon_password.title",
    fallbackTitle: "RCON Password",
    descriptionKey: "settings.schema.conanexiles.rcon_password.description",
    fallbackDescription: "Required when the remote console is enabled. LanGame writes it into Game.ini for this instance."
  },
  mod_workshop_ids: {
    titleKey: "settings.schema.conanexiles.mod_workshop_ids.title",
    fallbackTitle: "Workshop Mod IDs",
    descriptionKey: "settings.schema.conanexiles.mod_workshop_ids.description",
    fallbackDescription: "Steam Workshop items copied into ConanSandbox/Mods and written into modlist.txt in load order."
  }
};

export const CONAN_REGION_LABEL_KEYS: Record<string, { key: string; fallback: string }> = {
  "": {
    key: "conan.settings.regions.all",
    fallback: "Exiled Lands and Isle of Siptah"
  },
  IsleOfSiptah: {
    key: "conan.settings.regions.exiledLandsOnly",
    fallback: "Exiled Lands only"
  }
};

export function readConanNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function readConanBoolean(value: unknown): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value !== 0;
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["true", "1", "yes", "on"].includes(normalized)) return true;
    if (["false", "0", "no", "off", ""].includes(normalized)) return false;
  }
  return Boolean(value);
}

export function buildConanFieldCopy(
  key: string,
  t: TranslateFn,
  locale: string
): GuidedFieldCopy | undefined {
  const catalog = locale.toLowerCase().startsWith("zh")
    ? ZH_CN_CONAN_EXILES_MESSAGES
    : EN_US_CONAN_EXILES_MESSAGES;
  const catalogKey = `settings.schema.conanexiles.${key}`;
  const title = catalog[`${catalogKey}.title`];
  if (title) {
    return { title, description: catalog[`${catalogKey}.description`] ?? "" };
  }
  const entry = FIELD_COPY[key];
  if (!entry) return undefined;
  return {
    title: t(entry.titleKey, undefined, entry.fallbackTitle),
    description: entry.descriptionKey
      ? t(entry.descriptionKey, undefined, entry.fallbackDescription ?? "")
      : undefined
  };
}
