import type { ModuleDetails } from "../../types";

export type ModProviderKind = "steam" | "curseforge" | "nexus" | "modrinth" | "thunderstore" | "manual" | "native";

export interface ModWorkflowCatalogEntry {
  moduleId: string;
  provider: ModProviderKind;
  sourceLabel: string;
  primaryUrl?: string;
  steamDownloadMode?: "steamcmd-cache" | "not-wired";
  supportStatus?: "supported" | "manual_only" | "not_modelled";
  installScope?: "server" | "client_only";
  unsupportedReason?: string;
}

export const MOD_WORKFLOW_CATALOG: Record<string, ModWorkflowCatalogEntry> = {
  dontstarve: {
    moduleId: "dontstarve",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/app/322330/workshop/",
    steamDownloadMode: "steamcmd-cache"
  },
  projectzomboid: {
    moduleId: "projectzomboid",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/app/108600/workshop/",
    steamDownloadMode: "steamcmd-cache"
  },
  unturned: {
    moduleId: "unturned",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/app/304930/workshop/",
    steamDownloadMode: "steamcmd-cache"
  },
  corekeeper: {
    moduleId: "corekeeper",
    provider: "thunderstore",
    sourceLabel: "Thunderstore",
    primaryUrl: "https://thunderstore.io/c/core-keeper/",
    steamDownloadMode: "not-wired"
  },
  arksurvivalevolved: {
    moduleId: "arksurvivalevolved",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/app/346110/workshop/",
    steamDownloadMode: "steamcmd-cache"
  },
  arksurvivalascended: {
    moduleId: "arksurvivalascended",
    provider: "curseforge",
    sourceLabel: "CurseForge",
    primaryUrl: "https://www.curseforge.com/ark-survival-ascended",
    steamDownloadMode: "not-wired"
  },
  barotrauma: {
    moduleId: "barotrauma",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/app/602960/workshop/",
    steamDownloadMode: "steamcmd-cache"
  },
  minecraft: {
    moduleId: "minecraft",
    provider: "modrinth",
    sourceLabel: "Modrinth",
    primaryUrl: "https://modrinth.com/mods",
    steamDownloadMode: "not-wired"
  },
  valheim: {
    moduleId: "valheim",
    provider: "thunderstore",
    sourceLabel: "Thunderstore",
    primaryUrl: "https://thunderstore.io/c/valheim/",
    steamDownloadMode: "not-wired"
  },
  sevendaystodie: {
    moduleId: "sevendaystodie",
    provider: "nexus",
    sourceLabel: "Nexus Mods",
    primaryUrl: "https://www.nexusmods.com/7daystodie",
    steamDownloadMode: "not-wired"
  },
  rimworld: {
    moduleId: "rimworld",
    provider: "steam",
    sourceLabel: "Steam Workshop",
    primaryUrl: "https://steamcommunity.com/sharedfiles/filedetails/?id=3005289691",
    steamDownloadMode: "not-wired",
    supportStatus: "not_modelled",
    installScope: "client_only",
    unsupportedReason: "RimWorld Together Workshop packages are client-side dependencies and are not loaded by the dedicated server."
  },
  runescapedragonwilds: {
    moduleId: "runescapedragonwilds",
    provider: "manual",
    sourceLabel: "RuneScape: Dragonwilds mods",
    steamDownloadMode: "not-wired",
    supportStatus: "not_modelled",
    unsupportedReason: "Official 0.11 notes mention Shockbyte-hosted mod management, but no supported self-hosted Workshop source, server-side mod folder, launch argument, or enablement file has been verified for the dedicated server."
  }
};

export function moduleHasModWorkbench(moduleId?: string | null, moduleDetails?: ModuleDetails | null): boolean {
  if (!moduleId) {
    return false;
  }

  const catalogEntry = MOD_WORKFLOW_CATALOG[moduleId];
  const hasModuleDefinedModWorkflow = Boolean(
    moduleDetails?.workshop ||
    moduleDetails?.mods?.source ||
    moduleDetails?.mods?.manual_staging ||
    moduleDetails?.mods?.enablement
  );

  if (catalogEntry?.supportStatus === "not_modelled" && !hasModuleDefinedModWorkflow) {
    return false;
  }

  return Boolean(
    catalogEntry ||
    hasModuleDefinedModWorkflow
  );
}
