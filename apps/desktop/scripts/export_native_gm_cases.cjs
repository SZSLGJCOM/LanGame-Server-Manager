// Export the actual frontend builders for the opt-in native lifecycle probe.
const fs = require("node:fs");
const path = require("node:path");
const { transpileTypeScript } = require("./typescript_source_tools.cjs");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { buildGmToolCommand, getGmToolCatalog, getInitialGmToolValues } = require("../src/views/servers/gm-tools.ts");

const output = process.argv[2];
const repository = path.resolve(__dirname, "../../..");
if (!output || !path.isAbsolute(output)) throw new Error("Pass an absolute output file outside the repository");
const relative = path.relative(repository, output);
if (!relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)) {
  throw new Error("Native fixture output must stay outside the repository");
}
const cases = [];
function add(moduleId, toolId, patch = {}) {
  const tool = getGmToolCatalog(moduleId)?.tools.find((entry) => entry.id === toolId);
  if (!tool) throw new Error(`Missing tool ${moduleId}:${toolId}`);
  const values = { ...getInitialGmToolValues(tool), ...patch };
  const result = buildGmToolCommand(moduleId, toolId, values);
  if (result.error || !result.commands.length) {
    throw new Error(`Not a dispatchable native case: ${moduleId}:${toolId}: ${result.error}`);
  }
  cases.push({ moduleId, toolId, values, commands: result.commands,
    processKey: result.processKey, dispatchOptions: result.dispatchOptions });
}

for (const moduleId of ["arksurvivalevolved", "arksurvivalascended"]) {
  add(moduleId, "ark_set_time", { time: "12:30" });
  add(moduleId, "ark_destroy_wild_dinos");
}
for (const shard of ["master", "caves"]) {
  add("dontstarve", "dst_give_item_to_player", { shard, playerIndex: "2", prefab: "twigs", amount: "3" });
  add("dontstarve", "dst_give_item_to_player", { shard, allPlayers: "true", prefab: "log", amount: "2" });
  add("dontstarve", "dst_give_item_to_player", { shard, playerIndex: "2", prefab: "flint", amount: "2", placeInInventory: "false" });
  add("dontstarve", "dst_revive_player", { shard, playerIndex: "2" });
}
add("dontstarve", "dst_set_season", { season: "winter" });
add("dontstarve", "dst_set_season", { season: "spring" });
add("dontstarve", "dst_set_rain", { enabled: "true" });
add("dontstarve", "dst_set_rain", { enabled: "false" });
add("dontstarve", "dst_give_item_to_player", { playerIndex: "2", prefab: "lgsm_missing_prefab", amount: "1", expectedError: "Unknown prefab" });
add("dontstarve", "dst_give_item_to_player", { playerIndex: "999", prefab: "twigs", amount: "1", expectedError: "Player not on shard" });
add("dontstarve", "dst_give_item_to_player", { playerIndex: "2", prefab: "evergreen", amount: "1", expectedError: "Entity needs ground spawn" });
add("dontstarve", "dst_revive_player", { playerIndex: "1", expectedError: "Player is not a ghost" });
// Maintenance now owns saves and broadcasts. Export the current module action
// declarations; the backend independently rebuilds each command from that ID.
function maintenance(moduleId, actionId, toolId, target) {
  const source = fs.readFileSync(path.join(repository, "modules", moduleId, "module.toml"), "utf8");
  const actions = source.split("[[runtime.player_actions]]").slice(1).map(block =>
    Object.fromEntries([...block.split(/\r?\n\[/)[0].matchAll(/^([a-z_]+) = "([^"\r\n]*)"\r?$/gm)].map(match => [match[1], match[2]])));
  const action = actions.find(candidate => candidate.id === actionId);
  if (!action?.command_template) throw new Error(`Missing maintenance action: ${moduleId}:${actionId}`);
  cases.push({ moduleId, toolId, values: {}, commands: [action.command_template.replace("{{target}}", target ?? "")],
    processKey: action.process_key ?? null, dispatchOptions: { runtimeActionId: actionId, ...(target ? { runtimeActionTarget: target } : {}) } });
}
for (const moduleId of ["terraria", "projectzomboid", "minecraft", "palworld"]) {
  maintenance(moduleId, "save_world", `${moduleId === "projectzomboid" ? "zomboid" : moduleId}_save_world`);
}
for (const moduleId of ["terraria", "palworld"]) maintenance(moduleId, "broadcast", `${moduleId}_broadcast`, "LGSM_NATIVE_GM_BROADCAST");
fs.writeFileSync(output, `${JSON.stringify(cases, null, 2)}\n`, { flag: "w" });
console.log(`Exported ${cases.length} native cases to ${output}`);
