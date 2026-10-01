const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
function modelFor(id) {
  const schema = parseGuidedSettingsSchema({
    summary: { id, name: id },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules", id, "schema.json"), "utf8")
  });
  assert.equal(schema.parseError, null);
  return buildConfigurationWorkspaceModel(schema);
}
function fieldSections(model) {
  return Object.fromEntries(model.items.map((item) => [item.fieldKey, item.sectionId]));
}

test("DST room excludes rules and credentials while preserving native cluster controls", () => {
  const model = modelFor("dontstarve");
  const sections = fieldSections(model);
  assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey).sort(),
    ["cluster_name", "cluster_description", "max_players", "cluster_intention", "cluster_password", "cluster_language", "lan_only_cluster"].sort());
  for (const key of ["game_mode", "pvp", "vote_enabled"]) assert.equal(sections[key], "cluster-rules", key);
  for (const key of ["pause_when_empty", "tick_rate", "autosaver_enabled", "max_snapshots"]) assert.equal(sections[key], "cluster-runtime", key);
  for (const key of ["cluster_token", "friends_only", "whitelist_slots", "steam_group_only", "steam_group_id", "steam_group_admins"]) assert.equal(sections[key], "access", key);
  assert.equal(sections.shard_bind_ip, "network");
  assert.equal(sections.offline_cluster, "network");
  for (const key of ["admin_list", "whitelist", "blocklist"]) assert.equal(model.items.find((item) => item.fieldKey === key).owner, "player_access");
});
