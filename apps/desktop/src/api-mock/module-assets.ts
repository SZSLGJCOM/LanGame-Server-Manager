import type { ModuleDetails, ModuleSummary, PortBinding } from "../types";

import schemaAbioticfactor from "../../../../modules/abioticfactor/schema.json";
import schemaArksurvivalascended from "../../../../modules/arksurvivalascended/schema.json";
import schemaArksurvivalevolved from "../../../../modules/arksurvivalevolved/schema.json";
import schemaAstroneer from "../../../../modules/astroneer/schema.json";
import schemaBarotrauma from "../../../../modules/barotrauma/schema.json";
import schemaConanexiles from "../../../../modules/conanexiles/schema.json";
import schemaCorekeeper from "../../../../modules/corekeeper/schema.json";
import schemaDontstarve from "../../../../modules/dontstarve/schema.json";
import schemaEnshrouded from "../../../../modules/enshrouded/schema.json";
import schemaHumanitz from "../../../../modules/humanitz/schema.json";
import schemaMinecraft from "../../../../modules/minecraft/schema.json";
import schemaNecesse from "../../../../modules/necesse/schema.json";
import schemaNightingale from "../../../../modules/nightingale/schema.json";
import schemaPalworld from "../../../../modules/palworld/schema.json";
import schemaProjectzomboid from "../../../../modules/projectzomboid/schema.json";
import schemaReturntomoria from "../../../../modules/returntomoria/schema.json";
import schemaRimworld from "../../../../modules/rimworld/schema.json";
import schemaRomestead from "../../../../modules/romestead/schema.json";
import schemaRunescapedragonwilds from "../../../../modules/runescapedragonwilds/schema.json";
import schemaRust from "../../../../modules/rust/schema.json";
import schemaSatisfactory from "../../../../modules/satisfactory/schema.json";
import schemaScum from "../../../../modules/scum/schema.json";
import schemaSevendaystodie from "../../../../modules/sevendaystodie/schema.json";
import schemaSonsoftheforest from "../../../../modules/sonsoftheforest/schema.json";
import schemaSoulmask from "../../../../modules/soulmask/schema.json";
import schemaSquad from "../../../../modules/squad/schema.json";
import schemaTerraria from "../../../../modules/terraria/schema.json";
import schemaTheforest from "../../../../modules/theforest/schema.json";
import schemaUnturned from "../../../../modules/unturned/schema.json";
import schemaValheim from "../../../../modules/valheim/schema.json";
import schemaVrising from "../../../../modules/vrising/schema.json";
import schemaWindrose from "../../../../modules/windrose/schema.json";
import tomlAbioticfactor from "../../../../modules/abioticfactor/module.toml?raw";
import tomlArksurvivalascended from "../../../../modules/arksurvivalascended/module.toml?raw";
import tomlArksurvivalevolved from "../../../../modules/arksurvivalevolved/module.toml?raw";
import tomlAstroneer from "../../../../modules/astroneer/module.toml?raw";
import tomlBarotrauma from "../../../../modules/barotrauma/module.toml?raw";
import tomlConanexiles from "../../../../modules/conanexiles/module.toml?raw";
import tomlCorekeeper from "../../../../modules/corekeeper/module.toml?raw";
import tomlDontstarve from "../../../../modules/dontstarve/module.toml?raw";
import tomlEnshrouded from "../../../../modules/enshrouded/module.toml?raw";
import tomlHumanitz from "../../../../modules/humanitz/module.toml?raw";
import tomlMinecraft from "../../../../modules/minecraft/module.toml?raw";
import tomlNecesse from "../../../../modules/necesse/module.toml?raw";
import tomlNightingale from "../../../../modules/nightingale/module.toml?raw";
import tomlPalworld from "../../../../modules/palworld/module.toml?raw";
import tomlProjectzomboid from "../../../../modules/projectzomboid/module.toml?raw";
import tomlReturntomoria from "../../../../modules/returntomoria/module.toml?raw";
import tomlRimworld from "../../../../modules/rimworld/module.toml?raw";
import tomlRomestead from "../../../../modules/romestead/module.toml?raw";
import tomlRunescapedragonwilds from "../../../../modules/runescapedragonwilds/module.toml?raw";
import tomlRust from "../../../../modules/rust/module.toml?raw";
import tomlSatisfactory from "../../../../modules/satisfactory/module.toml?raw";
import tomlScum from "../../../../modules/scum/module.toml?raw";
import tomlSevendaystodie from "../../../../modules/sevendaystodie/module.toml?raw";
import tomlSonsoftheforest from "../../../../modules/sonsoftheforest/module.toml?raw";
import tomlSoulmask from "../../../../modules/soulmask/module.toml?raw";
import tomlSquad from "../../../../modules/squad/module.toml?raw";
import tomlTerraria from "../../../../modules/terraria/module.toml?raw";
import tomlTheforest from "../../../../modules/theforest/module.toml?raw";
import tomlUnturned from "../../../../modules/unturned/module.toml?raw";
import tomlValheim from "../../../../modules/valheim/module.toml?raw";
import tomlVrising from "../../../../modules/vrising/module.toml?raw";
import tomlWindrose from "../../../../modules/windrose/module.toml?raw";

export type MockSchemaProperty = {
  default?: unknown;
  maxLength?: number;
  "x-lsgm-generated-secret-length"?: number;
  "x-lsgm-default-source"?:
    | "instance_id"
    | "instance_name"
    | "generated_secret"
    | "official_unspecified";
};

export type MockSchemaObject = {
  type?: string;
  title?: string;
  properties?: Record<string, MockSchemaProperty>;
  required?: string[];
};

export const mockModuleSchemasById: Record<string, MockSchemaObject> = {
  abioticfactor: schemaAbioticfactor as MockSchemaObject,
  arksurvivalascended: schemaArksurvivalascended as MockSchemaObject,
  arksurvivalevolved: schemaArksurvivalevolved as MockSchemaObject,
  astroneer: schemaAstroneer as MockSchemaObject,
  barotrauma: schemaBarotrauma as MockSchemaObject,
  conanexiles: schemaConanexiles as MockSchemaObject,
  corekeeper: schemaCorekeeper as MockSchemaObject,
  dontstarve: schemaDontstarve as MockSchemaObject,
  enshrouded: schemaEnshrouded as MockSchemaObject,
  humanitz: schemaHumanitz as MockSchemaObject,
  minecraft: schemaMinecraft as MockSchemaObject,
  necesse: schemaNecesse as MockSchemaObject,
  nightingale: schemaNightingale as MockSchemaObject,
  palworld: schemaPalworld as MockSchemaObject,
  projectzomboid: schemaProjectzomboid as MockSchemaObject,
  returntomoria: schemaReturntomoria as MockSchemaObject,
  rimworld: schemaRimworld as MockSchemaObject,
  romestead: schemaRomestead as MockSchemaObject,
  runescapedragonwilds: schemaRunescapedragonwilds as MockSchemaObject,
  rust: schemaRust as MockSchemaObject,
  satisfactory: schemaSatisfactory as MockSchemaObject,
  scum: schemaScum as MockSchemaObject,
  sevendaystodie: schemaSevendaystodie as MockSchemaObject,
  sonsoftheforest: schemaSonsoftheforest as MockSchemaObject,
  soulmask: schemaSoulmask as MockSchemaObject,
  squad: schemaSquad as MockSchemaObject,
  terraria: schemaTerraria as MockSchemaObject,
  theforest: schemaTheforest as MockSchemaObject,
  unturned: schemaUnturned as MockSchemaObject,
  valheim: schemaValheim as MockSchemaObject,
  vrising: schemaVrising as MockSchemaObject,
  windrose: schemaWindrose as MockSchemaObject,
};

export const mockModuleTomlById: Record<string, string> = {
  abioticfactor: tomlAbioticfactor,
  arksurvivalascended: tomlArksurvivalascended,
  arksurvivalevolved: tomlArksurvivalevolved,
  astroneer: tomlAstroneer,
  barotrauma: tomlBarotrauma,
  conanexiles: tomlConanexiles,
  corekeeper: tomlCorekeeper,
  dontstarve: tomlDontstarve,
  enshrouded: tomlEnshrouded,
  humanitz: tomlHumanitz,
  minecraft: tomlMinecraft,
  necesse: tomlNecesse,
  nightingale: tomlNightingale,
  palworld: tomlPalworld,
  projectzomboid: tomlProjectzomboid,
  returntomoria: tomlReturntomoria,
  rimworld: tomlRimworld,
  romestead: tomlRomestead,
  runescapedragonwilds: tomlRunescapedragonwilds,
  rust: tomlRust,
  satisfactory: tomlSatisfactory,
  scum: tomlScum,
  sevendaystodie: tomlSevendaystodie,
  sonsoftheforest: tomlSonsoftheforest,
  soulmask: tomlSoulmask,
  squad: tomlSquad,
  terraria: tomlTerraria,
  theforest: tomlTheforest,
  unturned: tomlUnturned,
  valheim: tomlValheim,
  vrising: tomlVrising,
  windrose: tomlWindrose,
};

export const mockInstalledModuleIds = new Set([
  "abioticfactor",
  "corekeeper",
  "dontstarve",
  "minecraft",
  "necesse",
  "palworld",
  "projectzomboid",
  "sevendaystodie"
]);

export function readMockTomlString(block: string, key: string): string | null {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const multilineBasic = block.match(
    new RegExp(`^${escapedKey}\\s*=\\s*"""(?:\\r?\\n)?([\\s\\S]*?)"""`, "m")
  );
  if (multilineBasic) {
    return multilineBasic[1].replace(/\\"/g, "\"").replace(/\\\\/g, "\\");
  }
  const multilineLiteral = block.match(
    new RegExp(`^${escapedKey}\\s*=\\s*'''(?:\\r?\\n)?([\\s\\S]*?)'''`, "m")
  );
  if (multilineLiteral) {
    return multilineLiteral[1];
  }
  const basic = block.match(new RegExp(`^${escapedKey}\\s*=\\s*"((?:\\\\.|[^"])*)"`, "m"));
  if (basic) {
    return basic[1].replace(/\\"/g, "\"").replace(/\\\\/g, "\\");
  }
  const literal = block.match(new RegExp(`^${escapedKey}\\s*=\\s*'([^']*)'`, "m"));
  return literal?.[1] ?? null;
}

export function readMockTomlInteger(block: string, key: string): number | null {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = block.match(new RegExp(`^${escapedKey}\\s*=\\s*(\\d+)`, "m"));
  if (!match) {
    return null;
  }
  const value = Number.parseInt(match[1], 10);
  return Number.isFinite(value) ? value : null;
}

export function readMockTomlIntegerMap(block: string, key: string): Record<string, number> | null {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = block.match(new RegExp(`^${escapedKey}\\s*=\\s*\\{([^}]*)\\}`, "m"));
  if (!match) {
    return null;
  }

  const result: Record<string, number> = {};
  const entries = match[1].split(",").map((entry) => entry.trim()).filter(Boolean);
  for (const entry of entries) {
    const pair = entry.match(/^(?:([A-Za-z0-9_-]+)|"((?:\\.|[^"])*)"|'([^']*)')\s*=\s*(\d+)$/);
    if (!pair) {
      return null;
    }
    const entryKey = pair[1] ?? pair[2]?.replace(/\\"/g, "\"").replace(/\\\\/g, "\\") ?? pair[3];
    const value = Number.parseInt(pair[4], 10);
    if (!entryKey || !Number.isSafeInteger(value)) {
      return null;
    }
    result[entryKey] = value;
  }
  return result;
}

export function readMockTomlBoolean(block: string, key: string): boolean | null {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = block.match(new RegExp(`^${escapedKey}\\s*=\\s*(true|false)`, "m"));
  return match ? match[1] === "true" : null;
}

export function readMockTomlStringArray(block: string, key: string): string[] {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = block.match(new RegExp(`^${escapedKey}\\s*=\\s*\\[([\\s\\S]*?)\\]`, "m"));
  if (!match) {
    return [];
  }
  return Array.from(match[1].matchAll(/"((?:\\.|[^"])*)"/g), (item) =>
    item[1].replace(/\\"/g, "\"").replace(/\\\\/g, "\\")
  );
}

export function readMockTomlTable(toml: string, tableName: string): string {
  const escapedTableName = tableName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = toml.match(new RegExp(`^\\[${escapedTableName}\\]\\s*$`, "m"));
  if (!match || match.index === undefined) {
    return "";
  }
  const rest = toml.slice(match.index);
  const nextTableIndex = rest.slice(match[0].length).search(/\r?\n\[/);
  return nextTableIndex < 0 ? rest : rest.slice(0, match[0].length + nextTableIndex);
}

export function readMockTomlArrayTables(toml: string, tableName: string): string[] {
  const marker = `[[${tableName}]]`;
  return toml
    .split(new RegExp(`\\r?\\n(?=\\[\\[${tableName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\]\\])`))
    .filter((block) => block.trimStart().startsWith(marker))
    .map((block) => {
      const nextTableIndex = block.slice(marker.length).search(/\r?\n\[/);
      return nextTableIndex < 0 ? block : block.slice(0, marker.length + nextTableIndex);
    });
}

export function buildMockModuleSummaries(): ModuleSummary[] {
  return Object.entries(mockModuleTomlById)
    .map(([moduleId, toml]) => ({
      id: readMockTomlString(toml, "id") ?? moduleId,
      name: readMockTomlString(toml, "name") ?? moduleId,
      version: readMockTomlString(toml, "version") ?? "0.1.0",
      description: readMockTomlString(toml, "description"),
      steam_app_id: readMockTomlInteger(toml, "steam_app_id"),
      install_state: mockInstalledModuleIds.has(moduleId) ? "Installed" : "NotInstalled",
      supported_platforms: readMockTomlStringArray(toml, "supported_platforms")
    }))
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function parseMockDefaultPortsFromModuleToml(moduleId: string): PortBinding[] {
  const toml = mockModuleTomlById[moduleId];
  if (!toml) {
    return [];
  }

  return readMockTomlArrayTables(toml, "default_ports")
    .map((block) => {
      const name = readMockTomlString(block, "name");
      const protocol = readMockTomlString(block, "protocol");
      const port = readMockTomlInteger(block, "port");
      return name && protocol && port ? { name, protocol, port } : null;
    })
    .filter((port): port is PortBinding => Boolean(port));
}

export function parseMockPortRolesFromModuleToml(
  moduleId: string
): NonNullable<ModuleDetails["runtime"]["port_roles"]> {
  const toml = mockModuleTomlById[moduleId];
  if (!toml) {
    return [];
  }
  return readMockTomlArrayTables(toml, "runtime.port_roles")
    .map((block) => {
      const role = readMockTomlString(block, "role");
      if (role !== "player" && role !== "service") {
        return null;
      }
      return {
        role,
        port_names: readMockTomlStringArray(block, "port_names")
      };
    })
    .filter((role): role is NonNullable<ModuleDetails["runtime"]["port_roles"]>[number] => Boolean(role));
}
