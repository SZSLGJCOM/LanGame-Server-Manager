import type { RuntimeCommandDispatchOptions } from "../../types";
import { DONTSTARVE_SHARDS } from "../settings/modules/dontstarve-shards";
import type { GmToolDefinition } from "./gm-tools";
import { DST_GM_ITEM_OPTIONS, searchDstGmItemOptions, type DstGmItemOption } from "./dst-gm-item-catalog";
import { commandResult, errorResult, luaString, readDstPrefabId, readEnum, readInteger, type GmCommandBuildResult, type GmCommandBuilder } from "./gm-tool-values";

export function getDstPrefabOptions(): DstGmItemOption[] {
  return DST_GM_ITEM_OPTIONS;
}

export function searchDstPrefabOptions(query: string, limit = 80): DstGmItemOption[] {
  return searchDstGmItemOptions(query, limit);
}

const DST_STDIN_OPTIONS: RuntimeCommandDispatchOptions = { transport: "stdin", throwOnError: true };

// Rendered by the world map's tool panels (DstWorldMapWorkbench), not the generic form: fields only declare builder inputs and defaults.
export const DST_GM_TOOLS: GmToolDefinition[] = [
      {
        id: "dst_give_item_to_player", category: "Rewards", title: "Give or spawn item", submitLabel: "Give",
        description: "Give inventory items to, or spawn entities next to, the player selected on the map or every player on its shard.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true },
          { key: "amount", label: "Amount", type: "number", defaultValue: "20", min: 1, max: 999, step: 1, required: true },
          { key: "allPlayers", label: "All players", type: "checkbox", defaultValue: "false" },
          { key: "placeInInventory", label: "Put in inventory", type: "checkbox", defaultValue: "true" },
          { key: "prefab", label: "Item / Creature", type: "text", defaultValue: "log", required: true }
        ]
      },
      {
        id: "dst_revive_player", category: "Players", title: "Revive player", submitLabel: "Revive",
        description: "Revive the ghost of the player selected on the map.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true }
        ]
      },
      {
        id: "dst_set_season", category: "World", title: "Set season", submitLabel: "Set season",
        description: "Force the Master shard into a chosen season.",
        fields: [{ key: "season", label: "Season", type: "select", defaultValue: "autumn" }]
      },
      {
        id: "dst_set_rain", category: "World", title: "Set precipitation", submitLabel: "Set precipitation",
        description: "Start or stop precipitation on the Master shard; winter precipitation can be snow.",
        fields: [{ key: "enabled", label: "Precipitation", type: "select", defaultValue: "true" }]
      },
      {
        id: "dst_set_phase", category: "World", title: "Set time of day", submitLabel: "Set time of day",
        description: "Jump the Master clock to day, dusk or night, or to the start of the next day; Caves follow Master.",
        fields: [{ key: "phase", label: "Phase", type: "select", defaultValue: "day" }]
      },
      {
        id: "dst_restore_player", category: "Players", title: "Restore player", submitLabel: "Restore",
        description: "Fill health, hunger and sanity, dry off and reset body temperature of the selected player.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true }
        ]
      },
      {
        id: "dst_toggle_godmode", category: "Players", title: "Toggle god mode", submitLabel: "Toggle",
        description: "Toggle invincibility of the selected player.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true }
        ]
      },
      {
        id: "dst_teleport_to_player", category: "Players", title: "Teleport to player", submitLabel: "Teleport",
        description: "Move the selected player to another player on the same shard.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true },
          { key: "destinationId", label: "Destination player", type: "text", required: true }
        ]
      },
      {
        id: "dst_teleport_to_point", category: "Players", title: "Teleport to point", submitLabel: "Teleport",
        description: "Move the selected player to a walkable world position picked on the map.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true },
          { key: "x", label: "World X", type: "number", required: true },
          { key: "z", label: "World Z", type: "number", required: true }
        ]
      },
      {
        id: "dst_unlock_recipes", category: "Players", title: "Unlock all recipes", submitLabel: "Unlock",
        description: "Turn on free crafting (every recipe unlocked) for the selected player.",
        fields: [
          { key: "shard", label: "Shard", type: "select", defaultValue: "master" },
          { key: "playerId", label: "Player", type: "text", required: true }
        ]
      },
      {
        id: "dst_clear_ground_items", category: "World", title: "Clear dropped items", submitLabel: "Clear",
        description: "Remove loose items lying on the ground of one shard; irreplaceable, heavy, equippable, container, trap and living items stay.",
        fields: [{ key: "shard", label: "Shard", type: "select", defaultValue: "master" }]
      }
];

/** Every value is enum- or pattern-validated before it reaches Lua; the UI never supplies Lua text. */
export const DST_GM_BUILDERS: Record<string, GmCommandBuilder> = {
  dst_give_item_to_player(values) {
    const prefab = readDstPrefabId(values, "prefab", "Item / Creature");
    if (prefab.error) {
      return errorResult(prefab.error, DST_STDIN_OPTIONS);
    }
    const amount = readInteger(values, "amount", "Amount", 1, 999);
    if (amount.error) {
      return errorResult(amount.error, DST_STDIN_OPTIONS);
    }
    const shard = readDstShard(values);
    if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
    const processKey = shard.value;
    const allPlayers = values.allPlayers === "true";
    const placeInInventory = values.placeInInventory !== "false";
    const playerId = allPlayers ? { value: "", error: null } : readDstPlayerId(values);
    if (playerId.error) {
      return errorResult(playerId.error, DST_STDIN_OPTIONS);
    }
    return commandResult(
      [buildDstItemScript(prefab.value, amount.value, playerId.value, allPlayers, placeInInventory)],
      processKey,
      DST_STDIN_OPTIONS
    );
  },
  dst_revive_player(values) {
    const shard = readDstShard(values);
    if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
    const playerId = readDstPlayerId(values);
    if (playerId.error) {
      return errorResult(playerId.error, DST_STDIN_OPTIONS);
    }
    return commandResult([`${dstFindPlayer(playerId.value)}assert(p, "Player is not on this shard"); assert(p:HasTag("playerghost"), "Player is not a ghost"); p:PushEvent("respawnfromghost"); print("[LGSM] Revival requested")`], shard.value, DST_STDIN_OPTIONS);
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
  },
  dst_set_phase(values) {
    // components/clock.lua (master sim): ms_setphase takes a PHASE_NAMES entry; ms_nextcycle ends the night into the next day.
    const phase = readEnum(values, "phase", "Phase", ["day", "dusk", "night", "next"]);
    if (phase.error) {
      return errorResult(phase.error, DST_STDIN_OPTIONS);
    }
    const lua = phase.value === "next" ? `TheWorld:PushEvent("ms_nextcycle")` : `TheWorld:PushEvent("ms_setphase", ${luaString(phase.value)})`;
    return commandResult([lua], "master", DST_STDIN_OPTIONS);
  },
  dst_restore_player(values) {
    return dstPlayerAction(values, (p) => `local c=${p}.components;if c.health then c.health:SetPercent(1) end;if c.hunger then c.hunger:SetPercent(1) end;if c.sanity then c.sanity:SetPercent(1) end;if c.moisture then c.moisture:SetPercent(0) end;if c.temperature then c.temperature:SetTemperature(TUNING.STARTING_TEMP) end;print("[LGSM] Restored")`);
  },
  dst_toggle_godmode(values) {
    return dstPlayerAction(values, (p) => `local h=assert(${p}.components.health,"No health");h:SetInvincible(not h:IsInvincible());print("[LGSM] Invincible="..tostring(h:IsInvincible()))`);
  },
  dst_teleport_to_player(values) {
    const destination = readDstPlayerId({ playerId: values.destinationId ?? "" });
    if (destination.error) return errorResult(`Destination: ${destination.error}`, DST_STDIN_OPTIONS);
    if (destination.value === values.playerId) return errorResult("Choose a different destination player.", DST_STDIN_OPTIONS);
    return dstPlayerAction(values, (p) => `${dstFindPlayer(destination.value, "q")}assert(q,"Destination player is not on this shard");${dstTeleport(p, "q.Transform:GetWorldPosition()")}`);
  },
  dst_teleport_to_point(values) {
    const x = readCoordinate(values, "x"), z = readCoordinate(values, "z");
    if (x.error || z.error) return errorResult(x.error ?? z.error ?? "", DST_STDIN_OPTIONS);
    return dstPlayerAction(values, (p) => `assert(TheWorld.Map:IsPassableAtPoint(${x.value},0,${z.value}),"Destination is not walkable");${dstTeleport(p, `${x.value},0,${z.value}`)}`);
  },
  dst_unlock_recipes(values) {
    // components/builder.lua: GiveAllRecipes toggles freebuildmode, so only switch it on.
    return dstPlayerAction(values, (p) => `local b=assert(${p}.components.builder,"No builder");if not b.freebuildmode then b:GiveAllRecipes() end;${p}:PushEvent("techlevelchange");print("[LGSM] Free crafting on")`);
  },
  dst_clear_ground_items(values) {
    const shard = readDstShard(values);
    if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
    return commandResult([DST_CLEAR_GROUND_ITEMS], shard.value, DST_STDIN_OPTIONS);
  }
};

/** Klei user IDs (KU_...) from the map's connected-player snapshot address one player regardless of AllPlayers order. */
function readDstPlayerId(values: Record<string, string>): { value: string; error: string | null } {
  const value = String(values.playerId ?? "").trim();
  if (!value) return { value: "", error: "Select a player first." };
  if (!/^[A-Za-z0-9_-]{1,32}$/.test(value)) return { value: "", error: "Player ID is not a valid Klei user ID." };
  return { value, error: null };
}

function dstFindPlayer(playerId: string, name = "p"): string {
  return `local ${name};for _,c in ipairs(AllPlayers) do if c.userid==${luaString(playerId)} then ${name}=c end end;`;
}

function buildDstItemScript(prefab: string, amount: number, playerId: string, allPlayers: boolean, inventory: boolean): string {
  const validateInventory = inventory ? 'assert(p.components.inventory and not p:HasTag("playerghost"),"No inventory");' : "";
  // Bind the prefab once to keep the longest accepted ids inside the 512-byte console limit.
  const variables = allPlayers ? `local f,t=${luaString(prefab)},{};` : `local f=${luaString(prefab)};`;
  // Snapshot every target before spawning: a player prefab can append to AllPlayers.
  // networking.lua LookupPlayerInstByUserID matches only userid, without name/index fallback.
  const target = allPlayers
    ? `for _,p in ipairs(AllPlayers)do ${validateInventory}t[#t+1]=p end;assert(#t>0,"No players");`
    : `local p=assert(LookupPlayerInstByUserID(${luaString(playerId)}),"No player");${validateInventory}`;
  const place = inventory
    ? 'if not e.components.inventoryitem then e:Remove();error("Not an item") end;p.components.inventory:GiveItem(e,nil,p:GetPosition())'
    : "local x,y,z=p.Transform:GetWorldPosition();e.Transform:SetPosition(x,y,z)";
  // mainfunctions.lua PrefabExists: a hand-typed id that no loaded prefab (or mod) registers fails before anything spawns.
  const exists = 'assert(PrefabExists(f),"Unknown prefab");';
  const spawn = `for i=1,${amount} do local e=assert(SpawnPrefab(f),"Spawn failed");${place}end`;
  return `${variables}${exists}${target}${allPlayers ? `for _,p in ipairs(t)do ${spawn} end` : spawn};print("[LGSM] Done")`;
}

function readDstShard(values: Record<string, string>): { value: string; error: string | null } {
  return readEnum({ ...values, shard: values.shard ?? "master" }, "shard", "Shard", DONTSTARVE_SHARDS);
}

/** A living, non-ghost player on its own shard; the action body gets that player's Lua variable. */
function dstPlayerAction(values: Record<string, string>, body: (player: string) => string): GmCommandBuildResult {
  const shard = readDstShard(values);
  if (shard.error) return errorResult(shard.error, DST_STDIN_OPTIONS);
  const playerId = readDstPlayerId(values);
  if (playerId.error) return errorResult(playerId.error, DST_STDIN_OPTIONS);
  return commandResult([`${dstFindPlayer(playerId.value)}assert(p,"Player is not on this shard");assert(not p:HasTag("playerghost"),"Player is a ghost");${body("p")}`], shard.value, DST_STDIN_OPTIONS);
}

/** consolecommands.lua c_teleport: Physics:Teleport keeps the physics body in sync, Transform is the fallback. */
function dstTeleport(player: string, position: string): string {
  return `if ${player}.Physics then ${player}.Physics:Teleport(${position}) else ${player}.Transform:SetPosition(${position}) end;print("[LGSM] Teleported")`;
}

function readCoordinate(values: Record<string, string>, key: "x" | "z"): { value: string; error: string | null } {
  const raw = String(values[key] ?? "").trim(), value = Number(raw);
  if (!/^-?\d{1,5}(\.\d{1,2})?$/.test(raw) || Math.abs(value) > 10000) return { value: "", error: `World ${key.toUpperCase()} must be a number between -10000 and 10000.` };
  return { value: String(value), error: null };
}

// Loose ground items only: no owner, not in limbo, nothing irreplaceable (eyebone, ...), heavy, equippable, a container, a trap or alive.
const DST_CLEAR_GROUND_ITEMS = 'local n=0;for _,v in pairs(Ents) do local i=v.components and v.components.inventoryitem;'
  + 'if i and i.owner==nil and v:IsValid() and not v:IsInLimbo() and v.entity:GetParent()==nil and not v:HasOneOfTags({"irreplaceable","nonpotatable","heavy","trap","mine","player"})'
  + ' and v.components.equippable==nil and v.components.container==nil and v.components.health==nil then v:Remove();n=n+1 end end;print("[LGSM] Removed "..n.." dropped items")';
