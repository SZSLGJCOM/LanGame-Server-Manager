const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    return `${originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (module, filename) => {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(filename.slice(0, -4), "utf8"))};`, filename);
};

// Capture a controllable clock in MockLivePlayerStore without waiting for real expiry.
let clock = new Date(2026, 8, 27, 12, 34, 56).getTime();
const originalNow = Date.now;
let invokeMock;
try {
  Date.now = () => clock;
  ({ invokeMock } = require(path.resolve(__dirname, "../src/api-mock.ts")));
} finally {
  Date.now = originalNow;
}
test.before(async () => {
  const status = await invokeMock("ensure_steamcmd_ready", { operationId: "fixture-steamcmd-ready" });
  assert.equal(status.ready, true);
});
const { buildMockSevenDaysBanSettings } = require("../src/api-mock/seven-days-player-actions.ts");
const { resolveSelectedRosterIdentity } = require("../src/views/servers/player-center/player-access-selected-target.ts");
const { applyMockPlayerAccessMutation } = require("../src/api-mock/player-access.ts");

async function readDetails(instanceId) {
  return invokeMock("read_instance_details_from_storage", { instanceId });
}

async function createRunningServer(name) {
  await invokeMock("install_module_game", { moduleId: "sevendaystodie" });
  const created = await invokeMock("create_instance_record", { input: { module_id: "sevendaystodie", name } });
  await invokeMock("start_instance_process", { instanceId: created.summary.id });
  return created.summary.id;
}

function actionInput(instanceId, snapshot, actionId = "ban_player") {
  return {
    instance_id: instanceId,
    snapshot_id: snapshot.snapshot_id,
    player_key: snapshot.entries[0].player_key,
    action_id: actionId
  };
}

test("synthetic seven-days ban keeps account namespaces and native ten-year date semantics", () => {
  const player = { display_name: "Same name", identifiers: [
    { kind: "steam_id", value: "76561198000000001", stable: true },
    { kind: "session_id", value: "41", stable: false }
  ] };
  const account = { platform: "Steam", userid: "76561198000000001" };
  const unrelated = { platform: "EOS", userid: account.userid };
  const settings = {
    max_players: 8,
    blacklist_entries: [{ ...account, reason: "old" }, unrelated],
    admin_users: [{ ...account, permission_level: 0 }, unrelated],
    whitelist_users: [account],
    admin_groups: [{ steam_id: "103582791434672565" }]
  };
  const before = structuredClone(settings);
  const next = buildMockSevenDaysBanSettings(settings, player, new Date(2024, 1, 29, 3, 4, 5).getTime());
  assert.deepEqual(next.blacklist_entries, [
    { ...account, name: "Same name", unbandate: "2034-02-28 03:04:05", reason: "LanGame" }, unrelated
  ]);
  assert.deepEqual(next.admin_users, [unrelated]);
  assert.deepEqual(next.whitelist_users, settings.whitelist_users);
  assert.deepEqual(next.admin_groups, settings.admin_groups);
  assert.equal(next.max_players, 8);
  assert.deepEqual(settings, before, "building the update must not mutate the stored settings");
  for (const identities of [[], [{ kind: "session_id", value: "41", stable: false }], [
    ...player.identifiers, { kind: "eos_id", value: "deadbeef", stable: true }
  ]]) {
    assert.throws(() => buildMockSevenDaysBanSettings(settings, { ...player, identifiers: identities }, clock), /canonical account/);
  }
});

test("native short EOS accounts survive selection, synthetic ban and roster removal", () => {
  const property = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules/sevendaystodie/schema.json"), "utf8"))
    .properties.blacklist_entries;
  const player = { display_name: "Short EOS", identifiers: [{ kind: "eos_id", value: "deadbeef", stable: true }] };
  const selected = resolveSelectedRosterIdentity(player, { key: "blacklist_entries", property });
  assert.deepEqual(selected.rawValue, { platform: "EOS", userid: "deadbeef" });
  const settings = buildMockSevenDaysBanSettings({ blacklist_entries: [], admin_users: [] }, player, clock);
  const outcome = applyMockPlayerAccessMutation(property, settings.blacklist_entries, {
    operation: "remove", value: settings.blacklist_entries[0]
  }, true);
  assert.equal(outcome.liveTarget, "EOS_deadbeef");
  assert.equal(outcome.liveStatus, "sent_unverified");
  assert.deepEqual(outcome.value, []);
});

test("authorized online mock ban enters the persistent blacklist and the same entry can be unbanned", async () => {
  const instanceId = await createRunningServer("Seven days ban flow");
  const before = await readDetails(instanceId);
  const beforeSettings = JSON.parse(before.settings_json);
  const snapshot = await invokeMock("refresh_instance_live_players", { instanceId });
  const account = snapshot.entries[0].identifiers.find((identity) => identity.stable);
  const input = actionInput(instanceId, snapshot);
  // A caller receives a clone; changing it cannot redirect the authoritative action.
  snapshot.entries[0].display_name = "Injected name";
  account.value = "ffffffffffffffffffffffffffffffff";
  const result = await invokeMock("execute_instance_player_action", { input });
  assert.equal(result.status, "sent");
  const after = await readDetails(instanceId);
  const settings = JSON.parse(after.settings_json);
  const ban = settings.blacklist_entries.find((entry) => entry.platform === "EOS");
  assert.deepEqual(ban, {
    platform: "EOS", userid: "00000000000000000000000000000001",
    name: "HostAlice", unbandate: "2036-09-27 12:34:56", reason: "LanGame"
  });
  assert.deepEqual(settings.blacklist_entries[0], beforeSettings.blacklist_entries[0]);
  assert.deepEqual(settings.admin_users, []);
  const expectedSettings = { ...beforeSettings, blacklist_entries: settings.blacklist_entries, admin_users: [] };
  assert.deepEqual(settings, expectedSettings);
  assert.deepEqual({ ...after, settings_json: before.settings_json }, before);

  await assert.rejects(invokeMock("execute_instance_player_action", { input }), /no longer current/);
  assert.equal((await readDetails(instanceId)).settings_json, after.settings_json);
  const removal = await invokeMock("apply_instance_player_access_mutation", {
    input: { instanceId, fieldKey: "blacklist_entries", operation: "remove", value: ban }
  });
  assert.equal(removal.persistentStatus, "updated");
  assert.equal(removal.liveStatus, "sent_unverified");
  const unbanned = JSON.parse((await readDetails(instanceId)).settings_json);
  assert.deepEqual(unbanned.blacklist_entries, beforeSettings.blacklist_entries);
  assert.deepEqual(unbanned.admin_users, [], "unban must not restore former administrator access");
  await invokeMock("stop_instance_process", { instanceId });
});

test("invalid, expired and stopped mock player actions never persist a blacklist change", async () => {
  const instanceId = await createRunningServer("Seven days rejected ban");
  const snapshot = await invokeMock("refresh_instance_live_players", { instanceId });
  const input = actionInput(instanceId, snapshot);
  const before = (await readDetails(instanceId)).settings_json;
  for (const invalid of [
    { ...input, target: "EOS_deadbeef" },
    { ...input, action_id: "unban_player" },
    { ...input, player_key: "another-player" },
    { ...input, snapshot_id: "another-snapshot" }
  ]) {
    await assert.rejects(invokeMock("execute_instance_player_action", { input: invalid }));
    assert.equal((await readDetails(instanceId)).settings_json, before);
  }
  clock = snapshot.expires_at_unix_ms;
  await assert.rejects(invokeMock("execute_instance_player_action", { input }), /expired/);
  assert.equal((await readDetails(instanceId)).settings_json, before);
  const refreshed = await invokeMock("refresh_instance_live_players", { instanceId });
  await invokeMock("execute_instance_player_action", { input: actionInput(instanceId, refreshed, "kick_player") });
  assert.equal((await readDetails(instanceId)).settings_json, before, "kick does not create a ban");
  await invokeMock("stop_instance_process", { instanceId });
  await assert.rejects(invokeMock("execute_instance_player_action", { input }), /unavailable/);
  assert.equal((await readDetails(instanceId)).settings_json, before);
});
