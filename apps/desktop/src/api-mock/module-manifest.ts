import type { ModuleDetails, ModuleMinecraftDistributionDetails, ModulePlayerActionDetails } from "../types";
import {
  mockModuleTomlById,
  readMockTomlArrayTables,
  readMockTomlBoolean,
  readMockTomlInteger,
  readMockTomlIntegerMap,
  readMockTomlString,
  readMockTomlStringArray,
  readMockTomlTable
} from "./module-assets";

export function parseMockPlayerActionsFromModuleToml(moduleId: string): ModulePlayerActionDetails[] {
  const toml = mockModuleTomlById[moduleId];
  if (!toml) {
    return [];
  }

  return readMockTomlArrayTables(toml, "runtime.player_actions")
    .map((block) => {
      const action = {} as ModulePlayerActionDetails;
      action.id = readMockTomlString(block, "id") ?? "action";
      action.kind = readMockTomlString(block, "kind");
      action.label = readMockTomlString(block, "label") ?? action.id;
      action.label_zh_cn = readMockTomlString(block, "label_zh_cn");
      action.transport = readMockTomlString(block, "transport") ?? "stdin";
      action.command_template = readMockTomlString(block, "command_template") ?? "";
      action.target_label = readMockTomlString(block, "target_label");
      action.target_label_zh_cn = readMockTomlString(block, "target_label_zh_cn");
      action.target_placeholder = readMockTomlString(block, "target_placeholder");
      action.target_placeholder_zh_cn = readMockTomlString(block, "target_placeholder_zh_cn");
      action.target_required = readMockTomlBoolean(block, "target_required") ?? false;
      action.target_encoding = readMockTomlString(block, "target_encoding");
      action.role_values = readMockTomlStringArray(block, "role_values");
      action.process_key = readMockTomlString(block, "process_key");
      action.port_name = readMockTomlString(block, "port_name");
      action.password_setting_key = readMockTomlString(block, "password_setting_key");
      action.enabled_setting_key = readMockTomlString(block, "enabled_setting_key");
      action.destructive = readMockTomlBoolean(block, "destructive") ?? false;
      return action;
    });
}

export function parseMockPlayerQueryFromModuleToml(moduleId: string): ModuleDetails["runtime"]["player_query"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "runtime.player_query") : "";
  if (!table) {
    return null;
  }

  return {
    protocol: readMockTomlString(table, "protocol") ?? "none",
    port_names: readMockTomlStringArray(table, "port_names")
  };
}

export function parseMockPlayerCountSourceFromModuleToml(moduleId: string): ModuleDetails["runtime"]["player_count_source"] {
  const table = readMockTomlTable(mockModuleTomlById[moduleId] ?? "", "runtime");
  return readMockTomlString(table, "player_count_source") === "player_list" ? "player_list" : "player_query";
}

export function parseMockPortGroupsFromModuleToml(moduleId: string): NonNullable<ModuleDetails["runtime"]["port_groups"]> {
  const toml = mockModuleTomlById[moduleId] ?? "";
  return readMockTomlArrayTables(toml, "runtime.port_groups")
    .map((block) => {
      const memberOffsets = readMockTomlIntegerMap(block, "member_offsets");
      return {
        id: readMockTomlString(block, "id") ?? "",
        members: readMockTomlStringArray(block, "members"),
        ...(memberOffsets ? { member_offsets: memberOffsets } : {})
      };
    })
    .filter((group) => group.id.length > 0 && group.members.length >= 2);
}

export function parseMockBindAddressFromModuleToml(
  moduleId: string
): NonNullable<ModuleDetails["runtime"]["bind_address"]> {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "runtime.bind_address") : "";
  const mode = readMockTomlString(table, "mode");

  return {
    mode: mode === "strict" ? "strict" : "unsupported",
    port_names: readMockTomlStringArray(table, "port_names"),
    required_setting_key: readMockTomlString(table, "required_setting_key"),
    startup_timeout_ms: readMockTomlInteger(table, "startup_timeout_ms") ?? 60000
  };
}

export function parseMockRuntimeShutdownFromModuleToml(moduleId: string): ModuleDetails["runtime"]["shutdown"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "runtime.shutdown") : "";
  const commandTables = toml ? readMockTomlArrayTables(toml, "runtime.shutdown.commands") : [];
  if (!table && commandTables.length === 0) {
    return null;
  }

  return {
    grace_period_ms: readMockTomlInteger(table, "grace_period_ms") ?? 10000,
    commands: commandTables
      .map((block) => {
        const command = {
          transport: readMockTomlString(block, "transport") ?? "stdin",
          fallback_transport: readMockTomlString(block, "fallback_transport"),
          command: readMockTomlString(block, "command") ?? "",
          process_key: readMockTomlString(block, "process_key"),
          port_name: readMockTomlString(block, "port_name"),
          wait_after_ms: readMockTomlInteger(block, "wait_after_ms") ?? 0
        };
        return {
          ...command,
          ["password_setting_key"]: readMockTomlString(block, "password_setting_key"),
          ["enabled_setting_key"]: readMockTomlString(block, "enabled_setting_key")
        };
      })
      .filter((command) => command.command.trim().length > 0)
  };
}

export function parseMockPlayerManagementFromModuleToml(moduleId: string): ModuleDetails["runtime"]["player_management"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "player_management") : "";
  if (!table) {
    return null;
  }

  return {
    status: readMockTomlString(table, "status") ?? "unclassified",
    planned_surface: readMockTomlString(table, "planned_surface") ?? "unknown",
    reason: readMockTomlString(table, "reason") ?? "",
    verification: readMockTomlString(table, "verification") ?? ""
  };
}

function parseMockMinecraftDistributionsFromModuleToml(moduleId: string): ModuleMinecraftDistributionDetails[] {
  const toml = mockModuleTomlById[moduleId];
  const distributionTables = toml ? readMockTomlArrayTables(toml, "install.minecraft.distributions") : [];

  return distributionTables
    .map((table) => ({
      id: readMockTomlString(table, "id") ?? "",
      label: readMockTomlString(table, "label") ?? "",
      server_jar: readMockTomlString(table, "server_jar") ?? "",
      source: readMockTomlString(table, "source"),
      supports_mods: readMockTomlBoolean(table, "supports_mods") ?? false,
      supports_plugins: readMockTomlBoolean(table, "supports_plugins") ?? false,
      supports_datapacks: readMockTomlBoolean(table, "supports_datapacks") ?? true,
      supports_resource_packs: readMockTomlBoolean(table, "supports_resource_packs") ?? true,
    }))
    .filter((distribution) => distribution.id.trim().length > 0);
}

export function parseMockInstallFromModuleToml(moduleId: string): ModuleDetails["install"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "install") : "";
  const minecraftTable = toml ? readMockTomlTable(toml, "install.minecraft") : "";
  if (!table && !minecraftTable) {
    return { shared_game_dir: moduleId };
  }

  return {
    shared_game_dir: readMockTomlString(table, "shared_game_dir") ?? moduleId,
    download_url_windows: readMockTomlString(table, "download_url_windows"),
    source: readMockTomlString(table, "source"),
    verification_path: readMockTomlString(table, "verification_path"),
    minecraft: minecraftTable
      ? {
          version: readMockTomlString(minecraftTable, "version"),
          manifest_url: readMockTomlString(minecraftTable, "manifest_url"),
          server_jar: readMockTomlString(minecraftTable, "server_jar"),
          java_policy: readMockTomlString(minecraftTable, "java_policy"),
          default_distribution: readMockTomlString(minecraftTable, "default_distribution"),
          distributions: parseMockMinecraftDistributionsFromModuleToml(moduleId),
        }
      : null
  };
}

export function parseMockProcessFromModuleToml(moduleId: string): ModuleDetails["process"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "process") : "";
  if (!table) {
    return null;
  }

  return {
    executable: readMockTomlString(table, "executable") ?? "server.exe",
    args_template: readMockTomlStringArray(table, "args_template"),
    environment_template: Object.fromEntries(
      [...readMockTomlTable(toml, "process.environment_template").matchAll(/^([A-Za-z0-9_]+)\s*=\s*"([^"\r\n]*)"\s*$/gm)]
        .map((match) => [match[1], match[2]])
    ),
    working_directory_template: readMockTomlString(table, "working_directory_template"),
    window_policy: readMockTomlString(table, "window_policy") ?? "background",
    host_surface: readMockTomlString(table, "host_surface") ?? "managed_terminal",
    host_notes: readMockTomlString(table, "host_notes")
  };
}

export function parseMockWorkshopFromModuleToml(moduleId: string): ModuleDetails["workshop"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "workshop") : "";
  if (!table) {
    return null;
  }

  return {
    provider: readMockTomlString(table, "provider") ?? "steam",
    consumer_app_id: readMockTomlInteger(table, "consumer_app_id"),
    supports_collections: readMockTomlBoolean(table, "supports_collections") ?? false
  };
}

export function parseMockModsFromModuleToml(moduleId: string): ModuleDetails["mods"] {
  const toml = mockModuleTomlById[moduleId];
  if (!toml) {
    return null;
  }

  const sourceTable = readMockTomlTable(toml, "mods.source");
  const stagingTable = readMockTomlTable(toml, "mods.manual_staging");
  const enablementTable = readMockTomlTable(toml, "mods.enablement");
  if (!sourceTable && !stagingTable && !enablementTable) {
    return null;
  }

  return {
    source: sourceTable
      ? {
          provider: readMockTomlString(sourceTable, "provider") ?? "manual",
          label: readMockTomlString(sourceTable, "label") ?? "Mods",
          url: readMockTomlString(sourceTable, "url") ?? "",
          loaders: readMockTomlStringArray(sourceTable, "loaders"),
          game_versions: readMockTomlStringArray(sourceTable, "game_versions"),
          install_note: readMockTomlString(sourceTable, "install_note")
        }
      : null,
    manual_staging: stagingTable
      ? {
          target_template: readMockTomlString(stagingTable, "target_template") ?? "",
          target_label: readMockTomlString(stagingTable, "target_label") ?? "Mods",
          accepts: readMockTomlStringArray(stagingTable, "accepts")
        }
      : null,
    enablement: enablementTable
      ? {
          setting_key: readMockTomlString(enablementTable, "setting_key") ?? "",
          setting_label: readMockTomlString(enablementTable, "setting_label") ?? "Mods",
          id_strategy: readMockTomlString(enablementTable, "id_strategy"),
          reference_strategy: readMockTomlString(enablementTable, "reference_strategy"),
          reference_game_id: readMockTomlInteger(enablementTable, "reference_game_id")
        }
      : null
  };
}

export function parseMockStorageFromModuleToml(moduleId: string): ModuleDetails["storage"] {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "storage") : "";
  if (!table) {
    return null;
  }

  return {
    saves_path_template: readMockTomlString(table, "saves_path_template") ?? ""
  };
}

export function parseMockProgramSharingFromModuleToml(moduleId: string): "shared" | "independent" {
  const toml = mockModuleTomlById[moduleId];
  const table = toml ? readMockTomlTable(toml, "storage") : "";
  return readMockTomlString(table, "program_sharing") === "shared" ? "shared" : "independent";
}
