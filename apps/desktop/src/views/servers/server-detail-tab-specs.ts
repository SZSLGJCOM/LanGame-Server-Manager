import type { TranslateFn } from "../../i18n";
import type { ModuleDetails } from "../../types";
import type { ServerDetailTabSpec } from "./ServerDetailTabs";
import { moduleHasGmTools } from "./gm-tool-capability";
import { modWorkbenchAvailability } from "./mod-workbench-capability";

export function buildServerDetailTabSpecs(input: {
  moduleId: string;
  moduleDetails: ModuleDetails | null;
  t: TranslateFn;
  archived?: boolean;
}): ServerDetailTabSpec[] {
  const { moduleId, moduleDetails, t, archived = false } = input;
  const hasGmTools = moduleHasGmTools(moduleId);
  const modAvailability = modWorkbenchAvailability(moduleId, moduleDetails);
  return [
    { id: "runtime", label: t("servers.tabs.runtime", undefined, "运行"), icon: "terminal" },
    { id: "settings", label: t("servers.tabs.settings", undefined, "配置"), icon: "settings" },
    { id: "mods", label: t("servers.tabs.mods", undefined, "模组"), icon: "package",
      disabled: modAvailability !== "available",
      disabledReason: modAvailability === "client_only"
        ? t("servers.tabs.modsClientOnly", undefined, "Mods for this game are installed on each player's game client; the dedicated server has none to manage.")
        : modAvailability === "unsupported"
          ? t("servers.tabs.modsUnsupported", undefined, "LanGame does not offer server-side Mod management for this game.")
          : undefined },
    { id: "players", label: t("servers.tabs.players", undefined, "玩家"), icon: "users" },
    { id: "maintenance", label: t("servers.tabs.maintenance", undefined, "维护"), icon: "shield" },
    { id: "gm", label: t("servers.tabs.gmTools", undefined, "工具"), icon: "zap", disabled: !hasGmTools || archived,
      disabledReason: archived && hasGmTools
        ? t("servers.archives.workspace.restoreForTools", undefined, "Restore the instance to use server tools.")
        : t("servers.gmTools.unavailable", undefined, "This game does not currently provide server tools.") }
  ];
}
