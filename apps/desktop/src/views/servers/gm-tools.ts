import type { RuntimeCommandDispatchOptions } from "../../types";
import type { TranslateFn } from "../../i18n";
import {
  DST_GM_ITEM_OPTIONS,
  searchDstGmItemOptions,
  type DstGmItemOption
} from "./dst-gm-item-catalog";
import {
  ARK_GM_CREATURE_OPTIONS,
  searchArkGmCreatureOptions as searchArkGmCreatureCatalogOptions,
  type ArkGmCreatureOption
} from "./ark-gm-creature-catalog";
import {
  ARK_GM_ITEM_OPTIONS,
  ARK_GM_NUMERIC_ITEM_OPTIONS,
  searchArkGmItemOptions as searchArkGmItemCatalogOptions,
  searchArkGmNumericItemOptions as searchArkGmNumericItemCatalogOptions,
  type ArkGmItemOption
} from "./ark-gm-item-catalog";
export {
  localizeArkCatalogCategory,
  localizeArkCatalogName,
  localizeDstPrefabCategory,
  localizeDstPrefabCategoryTerm,
  localizeDstPrefabName
} from "./gm-catalog-i18n";

export type { ArkGmCreatureOption } from "./ark-gm-creature-catalog";
export type { ArkGmItemOption } from "./ark-gm-item-catalog";

export type GmToolFieldType = "text" | "number" | "select" | "textarea" | "checkbox";

export interface GmToolFieldOption {
  value: string;
  label: string;
  labelKey?: string;
  category?: string;
}

export interface ArkCustomCreatureEntry {
  label?: string | null;
  classId: string;
  modId?: string | null;
}

export interface GmToolField {
  key: string;
  label: string;
  type: GmToolFieldType;
  placeholder?: string;
  placeholderKey?: string;
  defaultValue?: string;
  required?: boolean;
  min?: number;
  max?: number;
  step?: number;
  options?: GmToolFieldOption[];
  visibleWhen?: {
    field: string;
    values: string[];
  };
}

export interface GmToolDefinition {
  id: string;
  category: string;
  title: string;
  description: string;
  submitLabel: string;
  fields: GmToolField[];
}

export interface GmToolCatalog {
  moduleIds: string[];
  title: string;
  description: string;
  tools: GmToolDefinition[];
}

export interface GmCommandBuildResult {
  commands: string[];
  processKey: string | null;
  dispatchOptions: RuntimeCommandDispatchOptions;
  preview: string;
  error: string | null;
}

type GmCommandBuilder = (
  values: Record<string, string>,
  t?: TranslateFn
) => GmCommandBuildResult;

type TranslateParams = Record<string, string | number | boolean | null | undefined>;

function localize(
  t: TranslateFn | undefined,
  key: string,
  fallback: string,
  params?: TranslateParams
): string {
  if (!t) {
    return fallback;
  }
  return t(key, params, fallback);
}

const EMPTY_RESULT: GmCommandBuildResult = {
  commands: [],
  processKey: null,
  dispatchOptions: {},
  preview: "",
  error: null
};

const ARK_REMOTE_ADMIN_OPTIONS: RuntimeCommandDispatchOptions = {
  transport: "source_rcon",
  portName: "rcon",
  passwordSettingKey: "admin_password",
  enabledSettingKey: "rcon_enabled",
  throwOnError: true
};

const DST_STDIN_OPTIONS: RuntimeCommandDispatchOptions = {
  transport: "stdin",
  throwOnError: true
};

const ARK_CREATURE_OPTIONS: GmToolFieldOption[] = ARK_GM_CREATURE_OPTIONS;
const ARK_ITEM_BLUEPRINT_OPTIONS: GmToolFieldOption[] = ARK_GM_ITEM_OPTIONS;
const ARK_ITEM_NUMERIC_OPTIONS: GmToolFieldOption[] = ARK_GM_NUMERIC_ITEM_OPTIONS;
const DST_PREFAB_OPTIONS: GmToolFieldOption[] = DST_GM_ITEM_OPTIONS;
const ARK_DEFAULT_ITEM_BLUEPRINT =
  "Blueprint'/Game/PrimalEarth/CoreBlueprints/Resources/PrimalItemResource_Stone.PrimalItemResource_Stone'";

const CATALOGS: GmToolCatalog[] = [
  {
    moduleIds: ["arksurvivalascended", "arksurvivalevolved"],
    title: "ARK GM Tools",
    description: "Spawn creatures, give player rewards, and control common world GM actions.",
    tools: [
      {
        id: "ark_spawn_creature",
        category: "Dinos",
        title: "Spawn creature",
        description: "Spawn a creature at a selected world position through the server's spawn integration.",
        submitLabel: "Spawn",
        fields: []
      },
      {
        id: "ark_give_item_to_player",
        category: "Rewards",
        title: "Give item to player",
        description: "Use one compact form for built-in blueprint paths, item numbers, or batch item-number reward lines.",
        submitLabel: "Give item",
        fields: [
          { key: "playerId", label: "In-game Player ID (not Steam ID)", type: "text", placeholder: "123456789", required: true },
          {
            key: "itemMode",
            label: "Item mode",
            type: "select",
            defaultValue: "blueprint",
            options: [
              { value: "blueprint", label: "Built-in item", labelKey: "servers.gmTools.options.arkItemMode.blueprint" },
              { value: "number", label: "Item number", labelKey: "servers.gmTools.options.arkItemMode.number" },
              { value: "batch", label: "Batch lines", labelKey: "servers.gmTools.options.arkItemMode.batch" }
            ]
          },
          {
            key: "itemId",
            label: "Item number",
            type: "number",
            placeholder: "9",
            defaultValue: "8",
            min: 1,
            step: 1,
            required: true,
            options: ARK_ITEM_NUMERIC_OPTIONS,
            visibleWhen: { field: "itemMode", values: ["number"] }
          },
          {
            key: "blueprintPath",
            label: "Item",
            type: "text",
            placeholder: "Blueprint'/Game/PrimalEarth/CoreBlueprints/Resources/PrimalItemResource_Stone.PrimalItemResource_Stone'",
            defaultValue: ARK_DEFAULT_ITEM_BLUEPRINT,
            required: true,
            options: ARK_ITEM_BLUEPRINT_OPTIONS,
            visibleWhen: { field: "itemMode", values: ["blueprint"] }
          },
          { key: "quantity", label: "Quantity", type: "number", defaultValue: "1", min: 1, max: 100000, step: 1, required: true, visibleWhen: { field: "itemMode", values: ["number", "blueprint"] } },
          { key: "quality", label: "Quality", type: "number", defaultValue: "1", min: 0, max: 1000, step: 1, required: true, visibleWhen: { field: "itemMode", values: ["number", "blueprint"] } },
          {
            key: "blueprint",
            label: "Blueprint",
            type: "select",
            defaultValue: "0",
            options: [
              { value: "0", label: "Item", labelKey: "servers.gmTools.options.blueprint.item" },
              { value: "1", label: "Blueprint", labelKey: "servers.gmTools.options.blueprint.blueprint" }
            ],
            visibleWhen: { field: "itemMode", values: ["number", "blueprint"] }
          },
          {
            key: "lines",
            label: "Reward lines",
            type: "textarea",
            placeholder: "9,50,1,0\n76,1,2,0",
            required: true,
            visibleWhen: { field: "itemMode", values: ["batch"] }
          }
        ]
      },
      {
        id: "ark_destroy_wild_dinos",
        category: "World",
        title: "Clear wild dinos",
        description: "Refresh wild creature spawns after changing map or difficulty settings.",
        submitLabel: "Clear wild dinos",
        fields: []
      },
      {
        id: "ark_set_time",
        category: "World",
        title: "Set time of day",
        description: "Move the world clock to a specific HH:MM time.",
        submitLabel: "Set time",
        fields: [
          { key: "time", label: "Time", type: "text", placeholder: "12:00", defaultValue: "12:00", required: true }
        ]
      }
    ]
  },
  {
    moduleIds: ["dontstarve"],
    title: "DST GM Tools",
    description: "Give items to inventory or spawn entities near players, revive players, and change season or weather.",
    tools: [
      {
        id: "dst_give_item_to_player",
        category: "Rewards",
        title: "Give or spawn item",
        description: "Use the selected shard's c_listallplayers() index to give inventory items or spawn entities nearby, for one player or everyone.",
        submitLabel: "Run item action",
        fields: [
          {
            key: "shard",
            label: "Shard",
            type: "select",
            defaultValue: "master",
            options: [
              { value: "master", label: "Master / Surface", labelKey: "servers.gmTools.options.shard.master" },
              { value: "caves", label: "Caves / Underground", labelKey: "servers.gmTools.options.shard.caves" }
            ]
          },
          { key: "playerIndex", label: "Player index", type: "number", defaultValue: "1", min: 1, step: 1, required: true },
          { key: "amount", label: "Amount", type: "number", defaultValue: "20", min: 1, max: 999, step: 1, required: true },
          { key: "allPlayers", label: "All players", type: "checkbox", defaultValue: "false" },
          { key: "placeInInventory", label: "Put in inventory", type: "checkbox", defaultValue: "true" },
          {
            key: "prefab",
            label: "Item / Creature",
            type: "text",
            placeholder: "log",
            defaultValue: "log",
            required: true,
            options: DST_PREFAB_OPTIONS
          }
        ]
      },
      {
        id: "dst_revive_player",
        category: "Players",
        title: "Revive player",
        description: "Revive a ghost on the selected shard using the player index from that shard's c_listallplayers().",
        submitLabel: "Revive",
        fields: [
          {
            key: "shard", label: "Shard", type: "select", defaultValue: "master",
            options: [
              { value: "master", label: "Master / Surface", labelKey: "servers.gmTools.options.shard.master" },
              { value: "caves", label: "Caves / Underground", labelKey: "servers.gmTools.options.shard.caves" }
            ]
          },
          { key: "playerIndex", label: "Player index", type: "number", defaultValue: "1", min: 1, step: 1, required: true }
        ]
      },
      {
        id: "dst_set_season",
        category: "World",
        title: "Set season",
        description: "Force the current shard world into a chosen season.",
        submitLabel: "Set season",
        fields: [
          {
            key: "season",
            label: "Season",
            type: "select",
            defaultValue: "autumn",
            options: [
              { value: "autumn", label: "Autumn", labelKey: "servers.gmTools.options.season.autumn" },
              { value: "winter", label: "Winter", labelKey: "servers.gmTools.options.season.winter" },
              { value: "spring", label: "Spring", labelKey: "servers.gmTools.options.season.spring" },
              { value: "summer", label: "Summer", labelKey: "servers.gmTools.options.season.summer" }
            ]
          }
        ]
      },
      {
        id: "dst_set_rain",
        category: "World",
        title: "Set precipitation",
        description: "Start or stop precipitation on the Master shard; winter precipitation can be snow.",
        submitLabel: "Set precipitation",
        fields: [
          {
            key: "enabled",
            label: "Precipitation",
            type: "select",
            defaultValue: "true",
            options: [
              { value: "true", label: "Start precipitation", labelKey: "servers.gmTools.options.rain.start" },
              { value: "false", label: "Stop precipitation", labelKey: "servers.gmTools.options.rain.stop" }
            ]
          }
        ]
      }
    ]
  }
];

function findGmToolCatalog(moduleId?: string | null): GmToolCatalog | null {
  const normalized = normalizeModuleId(moduleId);
  return CATALOGS.find((candidate) => candidate.moduleIds.includes(normalized)) ?? null;
}

const BUILDERS: Record<string, GmCommandBuilder> = {
  ark_give_item_to_player(values) {
    const playerId = readArkPlayerId(values);
    if (playerId.error) {
      return errorResult(playerId.error, ARK_REMOTE_ADMIN_OPTIONS);
    }
    const itemMode = readEnum(values, "itemMode", "Item mode", ["number", "blueprint", "batch"]);
    if (itemMode.error) {
      return errorResult(itemMode.error, ARK_REMOTE_ADMIN_OPTIONS);
    }
    if (itemMode.value === "number") {
      const single = buildArkGiveItemNumToPlayer(values);
      if (single.error) {
        return errorResult(single.error, ARK_REMOTE_ADMIN_OPTIONS);
      }
      return commandResult([single.command], null, ARK_REMOTE_ADMIN_OPTIONS);
    }
    if (itemMode.value === "blueprint") {
      const blueprintPath = readRequiredToken(values, "blueprintPath", "Blueprint path");
      if (blueprintPath.error) {
        return errorResult(blueprintPath.error, ARK_REMOTE_ADMIN_OPTIONS);
      }
      const quantity = readInteger(values, "quantity", "Quantity", 1, 100000);
      if (quantity.error) {
        return errorResult(quantity.error, ARK_REMOTE_ADMIN_OPTIONS);
      }
      const quality = readInteger(values, "quality", "Quality", 0, 1000);
      if (quality.error) {
        return errorResult(quality.error, ARK_REMOTE_ADMIN_OPTIONS);
      }
      const blueprint = readEnum(values, "blueprint", "Blueprint", ["0", "1"]);
      if (blueprint.error) {
        return errorResult(blueprint.error, ARK_REMOTE_ADMIN_OPTIONS);
      }
      return commandResult(
        [
          `GiveItemToPlayer ${playerId.value} ${jsonString(blueprintPath.value)} ${quantity.value} ${quality.value} ${blueprint.value}`
        ],
        null,
        ARK_REMOTE_ADMIN_OPTIONS
      );
    }
    return buildArkGiveItemBatchResult(playerId.value, values);
  },
  ark_destroy_wild_dinos() {
    return commandResult(["DestroyWildDinos"], null, ARK_REMOTE_ADMIN_OPTIONS);
  },
  ark_set_time(values) {
    const time = readRequiredToken(values, "time", "Time");
    if (time.error) {
      return errorResult(time.error, ARK_REMOTE_ADMIN_OPTIONS);
    }
    if (!/^(?:[01]?\d|2[0-3]):[0-5]\d$/.test(time.value)) {
      return errorResult("Time must be between 00:00 and 23:59 (HH:MM).", ARK_REMOTE_ADMIN_OPTIONS);
    }
    return commandResult([`SetTimeOfDay ${time.value}`], null, ARK_REMOTE_ADMIN_OPTIONS);
  },
  dst_give_item_to_player(values) {
    const prefab = readRequiredLuaPrefab(values, "prefab", "Item / Creature");
    if (prefab.error) {
      return errorResult(prefab.error, DST_STDIN_OPTIONS);
    }
    const amount = readInteger(values, "amount", "Amount", 1, 999);
    if (amount.error) {
      return errorResult(amount.error, DST_STDIN_OPTIONS);
    }
    const shard = readEnum({ ...values, shard: values.shard ?? "master" }, "shard", "Shard", ["master", "caves"]);
    if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
    const processKey = shard.value;
    const allPlayers = values.allPlayers === "true";
    const placeInInventory = values.placeInInventory !== "false";
    const playerIndex = readInteger({ ...values, playerIndex: allPlayers ? "1" : values.playerIndex }, "playerIndex", "Player index", 1, 999);
    if (playerIndex.error) {
      return errorResult(playerIndex.error, DST_STDIN_OPTIONS);
    }
    return commandResult(
      [buildDstItemScript(prefab.value, amount.value, playerIndex.value, allPlayers, placeInInventory)],
      processKey,
      DST_STDIN_OPTIONS
    );
  },
  dst_revive_player(values) {
    const shard = readEnum({ ...values, shard: values.shard ?? "master" }, "shard", "Shard", ["master", "caves"]);
    if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
    const playerIndex = readInteger(values, "playerIndex", "Player index", 1, 999);
    if (playerIndex.error) {
      return errorResult(playerIndex.error, DST_STDIN_OPTIONS);
    }
    return commandResult([`local p=AllPlayers[${playerIndex.value}]; assert(p, "Player is not on this shard"); assert(p:HasTag("playerghost"), "Player is not a ghost"); p:PushEvent("respawnfromghost"); print("[LGSM] Revival requested")`], shard.value, DST_STDIN_OPTIONS);
  },
  dst_set_season(values) {
    const season = readEnum(values, "season", "Season", ["autumn", "winter", "spring", "summer"]);
    if (season.error) {
      return errorResult(season.error, DST_STDIN_OPTIONS);
    }
    return commandResult([`TheWorld:PushEvent("ms_setseason", ${luaString(season.value)})`], "master", DST_STDIN_OPTIONS);
  },
  dst_set_rain(values) {
    const enabled = readEnum(values, "enabled", "Rain", ["true", "false"]);
    if (enabled.error) {
      return errorResult(enabled.error, DST_STDIN_OPTIONS);
    }
    return commandResult([`TheWorld:PushEvent("ms_forceprecipitation", ${enabled.value})`], "master", DST_STDIN_OPTIONS);
  }
};

export function moduleHasGmTools(moduleId?: string | null): boolean {
  return findGmToolCatalog(moduleId) !== null;
}

export function getArkGmCreatureOptions(): ArkGmCreatureOption[] {
  return ARK_GM_CREATURE_OPTIONS;
}

export function searchArkGmCreatureOptions(query: string, limit = 80): ArkGmCreatureOption[] {
  return searchArkGmCreatureCatalogOptions(query, limit);
}

export function getArkGmItemOptions(): ArkGmItemOption[] {
  return ARK_GM_ITEM_OPTIONS;
}

export function getArkGmNumericItemOptions(): ArkGmItemOption[] {
  return ARK_GM_NUMERIC_ITEM_OPTIONS;
}

export function searchArkGmItemOptions(query: string, limit = 80): ArkGmItemOption[] {
  return searchArkGmItemCatalogOptions(query, limit);
}

export function searchArkGmNumericItemOptions(query: string, limit = 80): ArkGmItemOption[] {
  return searchArkGmNumericItemCatalogOptions(query, limit);
}

export function getDstPrefabOptions(): DstGmItemOption[] {
  return DST_GM_ITEM_OPTIONS;
}

export function searchDstPrefabOptions(query: string, limit = 80): DstGmItemOption[] {
  return searchDstGmItemOptions(query, limit);
}

export function getGmToolCatalog(moduleId?: string | null): GmToolCatalog | null {
  return findGmToolCatalog(moduleId);
}

export function parseArkEnabledModIds(moduleId: string | null | undefined, settingsJson: string | null | undefined): string[] {
  const normalized = normalizeModuleId(moduleId);
  if (!isArkModuleId(normalized)) {
    return [];
  }
  const settings = parseSettingsJson(settingsJson);
  const key = normalized === "arksurvivalascended" ? "mod_ids_csv" : "active_mod_ids";
  return parseModIdList(settings[key]);
}

export function buildArkCreatureOptions(
  moduleId: string | null | undefined,
  settingsJson: string | null | undefined,
  customCreatures: ArkCustomCreatureEntry[] = [],
  t?: TranslateFn
): GmToolFieldOption[] {
  const normalized = normalizeModuleId(moduleId);
  const enabledModIds = new Set(parseArkEnabledModIds(normalized, settingsJson));
  const options = [...ARK_CREATURE_OPTIONS];
  const seen = new Set(options.map((option) => option.value.toLowerCase()));
  for (const entry of customCreatures) {
    const classId = String(entry.classId ?? "").trim();
    if (!classId || /[\r\n\t]/.test(classId)) {
      continue;
    }
    const dedupeKey = classId.toLowerCase();
    if (seen.has(dedupeKey)) {
      continue;
    }
    seen.add(dedupeKey);
    const label = String(entry.label ?? "").trim() || classId;
    const modId = String(entry.modId ?? "").trim();
    const source = modId
      ? enabledModIds.has(modId)
        ? localize(t, "servers.gmTools.arkCreatureMod", `Mod ${modId}`, { modId })
        : localize(t, "servers.gmTools.arkCreatureSavedMod", `Saved mod ${modId}`, { modId })
      : localize(t, "servers.gmTools.arkCreatureSavedSource", "Saved");
    options.push({
      value: classId,
      label: `${label} / ${source} / ${classId}`
    });
  }
  return options;
}

export function getInitialGmToolValues(tool: GmToolDefinition): Record<string, string> {
  return Object.fromEntries(tool.fields.map((field) => [field.key, field.defaultValue ?? ""]));
}

export function buildGmToolCommand(
  moduleId: string | null | undefined,
  toolId: string,
  values: Record<string, string>,
  t?: TranslateFn
): GmCommandBuildResult {
  const catalog = getGmToolCatalog(moduleId);
  if (!catalog) {
    return { ...EMPTY_RESULT, error: "This game does not have GM tools yet." };
  }
  if (!catalog.tools.some((tool) => tool.id === toolId)) {
    return { ...EMPTY_RESULT, error: "Unknown GM tool." };
  }
  if (toolId === "ark_spawn_creature") {
    return { ...EMPTY_RESULT, error: "Use the dedicated creature spawning panel for this action." };
  }
  const builder = BUILDERS[toolId];
  if (!builder) {
    return { ...EMPTY_RESULT, error: "GM tool is missing a command builder." };
  }
  let result = builder(values, t);
  if (result.commands.some((command) => new TextEncoder().encode(command).byteLength > 512)) {
    result = errorResult("Generated command is too long; shorten the input.", result.dispatchOptions);
  }
  return result;
}

function buildArkGiveItemNumToPlayer(values: Record<string, string>): { command: string; error: string | null } {
  const playerId = readArkPlayerId(values);
  if (playerId.error) {
    return { command: "", error: playerId.error };
  }
  const itemId = readInteger(values, "itemId", "Item ID", 1, 1000000);
  if (itemId.error) {
    return { command: "", error: itemId.error };
  }
  const quantity = readInteger(values, "quantity", "Quantity", 1, 100000);
  if (quantity.error) {
    return { command: "", error: quantity.error };
  }
  const quality = readInteger(values, "quality", "Quality", 0, 1000);
  if (quality.error) {
    return { command: "", error: quality.error };
  }
  const blueprint = readEnum(values, "blueprint", "Blueprint", ["0", "1"]);
  if (blueprint.error) {
    return { command: "", error: blueprint.error };
  }
  return {
    command: `GiveItemNumToPlayer ${playerId.value} ${itemId.value} ${quantity.value} ${quality.value} ${blueprint.value}`,
    error: null
  };
}

function buildArkGiveItemBatchResult(playerId: string, values: Record<string, string>): GmCommandBuildResult {
  const lines = String(values.lines ?? "")
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length === 0) {
    return errorResult("Reward lines are required.", ARK_REMOTE_ADMIN_OPTIONS);
  }
  if (lines.length > 64) {
    return errorResult("Send at most 64 reward lines at a time.", ARK_REMOTE_ADMIN_OPTIONS);
  }
  const commands: string[] = [];
  for (const [index, line] of lines.entries()) {
    if (line.split(",").length > 4) {
      return errorResult(`Line ${index + 1}: Expected item ID, quantity, quality, and blueprint only.`, ARK_REMOTE_ADMIN_OPTIONS);
    }
    const [itemId = "", quantity = "1", quality = "1", blueprint = "0"] = line.split(",").map((part) => part.trim());
    const single = buildArkGiveItemNumToPlayer({
      playerId,
      itemId,
      quantity,
      quality,
      blueprint
    });
    if (single.error) {
      return errorResult(`Line ${index + 1}: ${single.error}`, ARK_REMOTE_ADMIN_OPTIONS);
    }
    commands.push(single.command);
  }
  return commandResult(commands, null, ARK_REMOTE_ADMIN_OPTIONS);
}

function commandResult(
  commands: string[],
  processKey: string | null,
  dispatchOptions: RuntimeCommandDispatchOptions
): GmCommandBuildResult {
  return {
    commands,
    processKey,
    dispatchOptions,
    preview: commands.join("\n"),
    error: null
  };
}

function errorResult(error: string, dispatchOptions: RuntimeCommandDispatchOptions): GmCommandBuildResult {
  return {
    commands: [],
    processKey: null,
    dispatchOptions,
    preview: "",
    error
  };
}

function normalizeModuleId(moduleId?: string | null): string {
  return String(moduleId ?? "").trim().toLowerCase();
}

function isArkModuleId(moduleId: string): boolean {
  return moduleId === "arksurvivalascended" || moduleId === "arksurvivalevolved";
}

function parseSettingsJson(settingsJson: string | null | undefined): Record<string, unknown> {
  if (!settingsJson) {
    return {};
  }
  try {
    const parsed = JSON.parse(settingsJson);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

function parseModIdList(value: unknown): string[] {
  const raw = Array.isArray(value) ? value.join(",") : String(value ?? "");
  const ids = raw
    .split(/[\s,;]+/g)
    .map((part) => part.trim())
    .filter(Boolean);
  return Array.from(new Set(ids));
}

function readArkPlayerId(values: Record<string, string>): { value: string; error: string | null } {
  const value = String(values.playerId ?? "").trim();
  if (!/^\d{1,10}$/.test(value) || Number(value) > 4294967295) {
    return { value: "", error: "Enter the numeric in-game Player ID (0–4294967295), not a Steam ID." };
  }
  return { value, error: null };
}

function buildDstItemScript(prefab: string, amount: number, playerIndex: number, allPlayers: boolean, inventory: boolean): string {
  const target = allPlayers
    // Spawning a player prefab can append to AllPlayers. Iterate a snapshot so
    // an all-player action cannot extend its own target list while executing.
    ? 'local t={};for i,p in ipairs(AllPlayers) do t[i]=p end;assert(#t>0,"No players");'
    : `local p=AllPlayers[${playerIndex}];assert(p,"Player not on shard");local t={p};`;
  const validateInventory = inventory
    ? 'for _,p in ipairs(t) do assert(p.components.inventory and not p:HasTag("playerghost"),"No active inventory") end;'
    : "";
  const place = inventory
    ? 'if not e.components.inventoryitem then e:Remove();error("Entity needs ground spawn") end;p.components.inventory:GiveItem(e,nil,p:GetPosition());'
    : "local x,y,z=p.Transform:GetWorldPosition();e.Transform:SetPosition(x,y,z);";
  return `${target}${validateInventory}for _,p in ipairs(t) do for i=1,${amount} do local e=assert(SpawnPrefab(${luaString(prefab)}),"Unknown prefab");${place}end end;print("[LGSM] Done")`;
}

function readRequiredToken(
  values: Record<string, string>,
  key: string,
  label: string
): { value: string; error: string | null } {
  const value = String(values[key] ?? "").trim();
  if (!value) {
    return { value: "", error: `${label} is required.` };
  }
  if (/[\x00-\x1f\x7f]/.test(value)) {
    return { value: "", error: `${label} must be a single-line value.` };
  }
  return { value, error: null };
}

function readRequiredLuaPrefab(
  values: Record<string, string>,
  key: string,
  label: string
): { value: string; error: string | null } {
  const token = readRequiredToken(values, key, label);
  if (token.error) {
    return token;
  }
  if (!/^[A-Za-z0-9_./-]+$/.test(token.value)) {
    return { value: "", error: `${label} can only contain letters, numbers, underscore, dash, slash, or dot.` };
  }
  return token;
}

function readInteger(
  values: Record<string, string>,
  key: string,
  label: string,
  min: number,
  max: number
): { value: number; error: string | null } {
  const raw = String(values[key] ?? "").trim();
  if (!raw) {
    return { value: min, error: `${label} is required.` };
  }
  const value = Number(raw);
  if (!Number.isInteger(value)) {
    return { value: min, error: `${label} must be an integer.` };
  }
  if (value < min || value > max) {
    return { value: min, error: `${label} must be between ${min} and ${max}.` };
  }
  return { value, error: null };
}

function readEnum(
  values: Record<string, string>,
  key: string,
  label: string,
  allowed: string[]
): { value: string; error: string | null } {
  const value = String(values[key] ?? "").trim();
  if (!allowed.includes(value)) {
    return { value: "", error: `${label} is not supported.` };
  }
  return { value, error: null };
}

function luaString(value: string): string {
  return JSON.stringify(value);
}

function jsonString(value: string): string {
  return JSON.stringify(value);
}
