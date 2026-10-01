const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}

const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));
test.before(async () => {
  const status = await invokeMock("ensure_steamcmd_ready", { operationId: "fixture-steamcmd-ready" });
  assert.equal(status.ready, true);
});
const {
  MockLivePlayerStore,
  parseMockPlayerListFromModuleToml
} = require(path.join(desktopRoot, "src", "api-mock", "live-players.ts"));

const dstPlayerList = parseMockPlayerListFromModuleToml("dontstarve");
assert.ok(dstPlayerList, "DST mock manifest must declare runtime.player_list");

async function updatePlayerCountSettings(instanceId, patch) {
  const details = await invokeMock("read_instance_details_from_storage", { instanceId });
  return invokeMock("update_instance_record_if_current", {
    expectedSettingsJson: details.settings_json,
    input: {
      id: instanceId, bind_ip: details.summary.bind_ip,
      auto_backup_on_stop: details.auto_backup_on_stop,
      backup_retention_count: details.backup_retention_count,
      ports: details.ports,
      settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), ...patch })
    }
  });
}

for (const moduleId of ["humanitz", "squad"]) {
  test(`${moduleId} mock count uses the declared current player list without a recursive count seed`, async () => {
    const module = await invokeMock("read_module_details", { moduleId });
    assert.equal(module.runtime.player_count_source, "player_list");
    assert.equal(module.runtime.player_query.protocol, "none");
    await invokeMock("install_module_game", { moduleId });
    const provisioning = await invokeMock("create_instance_record", {
      input: { name: `${moduleId} count contract`, module_id: moduleId }
    });
    const instanceId = provisioning.summary.id;
    const readCount = () => invokeMock("read_instance_runtime_overview_from_storage", { instanceId });
    assert.equal((await readCount()).players.current_players, null);
    assert.equal((await readCount()).players.query.status, "stopped");
    await updatePlayerCountSettings(instanceId, { rcon_enabled: true, rcon_password: "synthetic-test-only" });
    await invokeMock("start_instance_process", { instanceId });

    const counted = (await readCount()).players;
    const listed = await invokeMock("read_instance_live_players", { instanceId });
    assert.equal(counted.query.status, "ready");
    assert.equal(listed.status, "ready");
    assert.equal(listed.complete, true);
    assert.equal(counted.current_players, listed.entries.length);
    assert.equal(counted.current_players, listed.current_players);
    assert.equal(counted.current_players, 2);
    await readCount();
    assert.equal((await invokeMock("read_instance_live_players", { instanceId })).snapshot_id, listed.snapshot_id,
      "Reading counts must not invalidate an already-current player action snapshot");

    await updatePlayerCountSettings(instanceId, { rcon_password: "" });
    const noPassword = (await readCount()).players;
    assert.equal(noPassword.current_players, null);
    assert.equal(noPassword.query.status, "unavailable");
    if (moduleId === "humanitz") {
      await updatePlayerCountSettings(instanceId, { rcon_enabled: false, rcon_password: "synthetic-test-only" });
      const disabled = (await readCount()).players;
      assert.equal(disabled.current_players, null);
      assert.equal(disabled.query.status, "unavailable");
    }
    await updatePlayerCountSettings(instanceId, { rcon_enabled: true, rcon_password: "synthetic-test-only" });
    const refreshed = await invokeMock("refresh_instance_live_players", { instanceId });
    assert.equal((await readCount()).players.current_players, refreshed.entries.length);
    await invokeMock("stop_instance_process", { instanceId });
    assert.equal((await readCount()).players.current_players, null);
  });
}

test("modules without an explicit count source retain their query protocol", async () => {
  const moduleId = "arksurvivalevolved";
  const module = await invokeMock("read_module_details", { moduleId });
  assert.equal(module.runtime.player_count_source, "player_query");
  assert.equal(module.runtime.player_query.protocol, "a2s_info");
  await invokeMock("install_module_game", { moduleId });
  const provisioned = await invokeMock("create_instance_record", { input: { module_id: moduleId, name: "Query count contract" } });
  const instanceId = provisioned.summary.id;
  await invokeMock("start_instance_process", { instanceId });
  const overview = await invokeMock("read_instance_runtime_overview_from_storage", { instanceId });
  assert.equal(overview.players.query.status, "ready");
  assert.equal(overview.players.current_players, 1);
  await invokeMock("stop_instance_process", { instanceId });
});

function dstContext(instanceId, currentPlayers = 2, running = true) {
  return {
    instance_id: instanceId,
    module_id: "dontstarve",
    settings: {},
    running,
    player_list: dstPlayerList,
    current_players: currentPlayers,
    max_players: 6
  };
}

function sampleEntry(playerKey = "klei_user_id:KU_test") {
  return {
    player_key: playerKey,
    display_name: "Test Player",
    identifiers: [{ kind: "klei_user_id", value: "KU_test", stable: true }],
    available_action_ids: ["kick_userid"],
    ping_ms: null,
    session_started_at_unix_ms: null,
    role: null,
    attributes: []
  };
}

function assertPublicOutputOnly(value) {
  const serialized = JSON.stringify(value);
  for (const forbidden of [
    "private_binding",
    "rendered_target",
    "command_template",
    "password_setting_key",
    "raw_response",
    "log_lines"
  ]) {
    assert.ok(!serialized.includes(forbidden), `mock output leaked ${forbidden}`);
  }
}

test("running DST returns deterministic ready and authoritative-empty snapshots", () => {
  const store = new MockLivePlayerStore({ now: () => 1_000 });
  const initial = store.read(dstContext("dst-ready", 2));
  assert.equal(initial.status, "refreshing");
  assert.equal(initial.complete, false);
  assert.deepEqual(initial.entries, []);
  const ready = store.refresh(dstContext("dst-ready", 2));
  assert.equal(ready.status, "ready");
  assert.equal(ready.complete, true);
  assert.equal(ready.truncated, false);
  assert.equal(ready.stale, false);
  assert.equal(ready.entries.length, 2);
  assert.equal(ready.current_players, 2);
  assert.deepEqual(ready.entries.map((entry) => entry.player_key), [
    `player:${ready.snapshot_id}:1`,
    `player:${ready.snapshot_id}:2`
  ]);

  const empty = store.refresh(dstContext("dst-empty", 0));
  assert.equal(empty.status, "ready");
  assert.equal(empty.complete, true);
  assert.equal(empty.current_players, 0);
  assert.deepEqual(empty.entries, []);
  assertPublicOutputOnly([ready, empty]);
});

test("stopped and unsupported modules expose capability truth without identities", () => {
  const store = new MockLivePlayerStore();
  const stopped = store.read(dstContext("dst-stopped", 2, false));
  assert.equal(stopped.status, "stopped");
  assert.equal(stopped.source, "structured_log");
  assert.equal(stopped.current_players, null);
  assert.deepEqual(stopped.entries, []);

  const unsupported = store.read({
    instance_id: "unsupported-with-count",
    module_id: "minecraft",
    settings: {},
    running: true,
    player_list: null,
    current_players: 7,
    max_players: 20
  });
  assert.equal(unsupported.status, "unsupported");
  assert.equal(unsupported.source, null);
  assert.equal(unsupported.current_players, 7);
  assert.equal(unsupported.max_players, 20);
  assert.deepEqual(unsupported.entries, []);
  assertPublicOutputOnly([stopped, unsupported]);
});

test("private Steam query settings do not invent online players in the mock", () => {
  const store = new MockLivePlayerStore();
  for (const [moduleId, key, disabled, enabled] of [
    ["valheim", "public_server", 0, 1],
    ["abioticfactor", "lan_only", true, false],
    ["vrising", "list_on_steam", false, true]
  ]) {
    const context = {
      ...dstContext(`visibility-${moduleId}`, 2), module_id: moduleId,
      player_list: parseMockPlayerListFromModuleToml(moduleId), settings: { [key]: disabled }
    };
    const blocked = store.refresh(context);
    assert.equal(blocked.status, "unsupported");
    assert.equal(blocked.current_players, null);
    assert.equal(blocked.complete, false);
    assert.deepEqual(blocked.entries, []);
    assert.deepEqual(blocked.issue.setting_keys, [key]);
    const visible = store.refresh({ ...context, settings: { [key]: enabled } });
    assert.equal(visible.status, "ready");
  }
});

test("refreshes are monotonic, isolated per instance, and clone returned values", () => {
  const store = new MockLivePlayerStore({ now: () => 2_000 });
  const first = store.refresh(dstContext("dst-isolated-a"));
  const second = store.refresh(dstContext("dst-isolated-a"));
  const other = store.refresh(dstContext("dst-isolated-b", 1));

  assert.notEqual(first.snapshot_id, second.snapshot_id);
  assert.ok(second.observed_at_unix_ms > first.observed_at_unix_ms);
  assert.notEqual(second.snapshot_id, other.snapshot_id);
  assert.equal(other.instance_id, "dst-isolated-b");
  assert.equal(other.entries.length, 1);

  second.entries[0].display_name = "mutated outside store";
  second.entries.length = 0;
  const reread = store.read(dstContext("dst-isolated-a"));
  assert.equal(reread.entries.length, 2);
  assert.equal(reread.entries[0].display_name, "HostAlice");
});

test("failed refresh falls back to stale last-success data and truncation is never promoted", () => {
  let attempt = 0;
  const staleStore = new MockLivePlayerStore({
    now: () => 3_000,
    collector: () => {
      attempt += 1;
      return attempt === 1
        ? {
            status: "ready",
            entries: [sampleEntry()],
            complete: true,
            truncated: false,
            issue: null,
            current_players: 1
          }
        : {
            status: "failed",
            entries: [],
            complete: false,
            truncated: false,
            issue: { code: "io_failed", setting_keys: [], summary: "Collection failed safely." },
            current_players: null
          };
    }
  });
  const success = staleStore.refresh(dstContext("dst-stale", 1));
  const failed = staleStore.refresh(dstContext("dst-stale", 1));
  assert.equal(success.status, "ready");
  assert.equal(failed.status, "failed");
  assert.equal(failed.stale, true);
  assert.equal(failed.complete, false);
  assert.equal(failed.issue.code, "io_failed");
  assert.deepEqual(
    failed.entries.map((entry) => entry.available_action_ids),
    [[]],
    "stale rows must not retain action authority"
  );
  assert.equal(failed.entries[0].player_key, success.entries[0].player_key);

  const truncatedStore = new MockLivePlayerStore({
    collector: () => ({
      status: "failed",
      entries: [sampleEntry("partial")],
      complete: false,
      truncated: true,
      issue: null,
      current_players: 2
    })
  });
  const truncated = truncatedStore.refresh(dstContext("dst-truncated", 2));
  assert.equal(truncated.status, "failed");
  assert.equal(truncated.complete, false);
  assert.equal(truncated.truncated, true);
  assert.equal(truncated.stale, false);
  assert.equal(truncated.issue.code, "capture_limit");
  assert.deepEqual(truncated.entries, []);
  assert.deepEqual(truncatedStore.read(dstContext("dst-truncated", 2)), truncated);
  assertPublicOutputOnly([failed, truncated]);
});

test("expired mock snapshots preserve display rows but revoke every row action", () => {
  let now = 10_000;
  const store = new MockLivePlayerStore({ now: () => now });
  const context = dstContext("dst-expired", 1);
  const ready = store.refresh(context);
  now = ready.expires_at_unix_ms;

  const expired = store.read(context);
  assert.equal(expired.status, "ready");
  assert.equal(expired.stale, true);
  assert.equal(expired.entries.length, 1);
  assert.deepEqual(expired.entries[0].available_action_ids, []);
  assert.throws(
    () => store.execute(context, {
      instance_id: context.instance_id,
      snapshot_id: ready.snapshot_id,
      player_key: ready.entries[0].player_key,
      action_id: "kick_userid"
    }),
    /expired/
  );
});

test("invokeMock projects each module's TOML capability without a game-id gate", async () => {
  const dstModule = await invokeMock("read_module_details", { moduleId: "dontstarve" });
  const minecraftModule = await invokeMock("read_module_details", { moduleId: "minecraft" });
  assert.deepEqual(dstModule.runtime.player_list, {
    scope: "online",
    source: "structured_log",
    action_id: "list_online_players",
    player_action_ids: ["kick_userid"],
    response_codec: "dst_client_table_v1",
    identity_kind: "klei_user_id",
    refresh_interval_ms: 30000
  });
  assert.deepEqual(minecraftModule.runtime.player_list, parseMockPlayerListFromModuleToml("minecraft"));

  const dst = await invokeMock("refresh_instance_live_players", {
    instanceId: "srv-dst-1",
    instance_id: "srv-dst-1"
  });
  const minecraft = await invokeMock("read_instance_live_players", {
    instanceId: "srv-minecraft-1",
    instance_id: "srv-minecraft-1"
  });
  const stoppedProvisioning = await invokeMock("create_instance_record", {
    input: { name: "Stopped DST contract", module_id: "dontstarve" }
  });
  const stopped = await invokeMock("read_instance_live_players", {
    instanceId: stoppedProvisioning.summary.id,
    instance_id: stoppedProvisioning.summary.id
  });

  assert.equal(dst.status, "ready");
  assert.equal(dst.entries.length, 2);
  assert.equal(minecraft.status, minecraftModule.runtime.player_list ? "refreshing" : "unsupported");
  assert.deepEqual(minecraft.entries, []);
  assert.equal(stopped.status, "stopped");
  assert.deepEqual(stopped.entries, []);
  assertPublicOutputOnly([dst, minecraft, stopped]);
});

test("invokeMock enforces snapshot and row action allowlists with the four-field input", async () => {
  const provisioning = await invokeMock("create_instance_record", {
    input: { name: "Action-isolated DST", module_id: "dontstarve" }
  });
  const instanceId = provisioning.summary.id;
  await invokeMock("start_instance_process", { instanceId, instance_id: instanceId });
  const snapshot = await invokeMock("refresh_instance_live_players", { instanceId, instance_id: instanceId });
  const player = snapshot.entries[0];
  const input = {
    instance_id: instanceId,
    snapshot_id: snapshot.snapshot_id,
    player_key: player.player_key,
    action_id: "kick_userid"
  };

  await assert.rejects(
    invokeMock("execute_instance_player_action", { input: { ...input, action_id: "ban_userid" } }),
    /not declared/
  );
  await assert.rejects(
    invokeMock("execute_instance_player_action", { input: { ...input, target: "KU_injected" } }),
    /four-field input/
  );

  const result = await invokeMock("execute_instance_player_action", { input });
  assert.deepEqual(Object.keys(result).sort(), ["action_id", "executed_at_unix_ms", "status", "summary"]);
  assert.equal(result.action_id, "kick_userid");
  assert.equal(result.status, "sent");
  const refreshing = await invokeMock("read_instance_live_players", { instanceId, instance_id: instanceId });
  assert.equal(refreshing.status, "refreshing");
  assert.equal(refreshing.stale, true);
  assert.equal(refreshing.complete, false);
  assert.notEqual(refreshing.snapshot_id, snapshot.snapshot_id);
  assert.deepEqual(refreshing.entries, []);
  assertPublicOutputOnly([result, refreshing]);
});

test("mock adapters cover declared transports and keep name-only rows read-only", () => {
  const store = new MockLivePlayerStore({ now: () => 1_000 });
  for (const [source, codec, identity] of [
    ["runtime_action", "rust_player_list", "steam_id"],
    ["http_api", "palworld_players", "palworld_user_id"],
    ["server_query", "a2s_players", "player_name"]
  ]) {
    const context = {
      ...dstContext(`generic-${source}`, 1), module_id: "generic-fixture",
      player_list: { ...dstPlayerList, source, response_codec: codec, identity_kind: identity, player_action_ids: identity === "player_name" ? [] : ["kick"] }
    };
    const ready = store.refresh(context);
    assert.equal(ready.status, "ready");
    assert.equal(ready.source, source);
    assert.equal(ready.entries.length, 1);
    if (identity === "player_name") {
      assert.deepEqual(ready.entries[0].identifiers, []);
      assert.deepEqual(ready.entries[0].available_action_ids, []);
    } else {
      assert.equal(ready.entries[0].identifiers[0].kind, identity);
      assert.deepEqual(ready.entries[0].available_action_ids, ["kick"]);
    }
    assertPublicOutputOnly(ready);
  }
});

test("mock username bindings retain declared moderation while query names remain read-only", () => {
  const store = new MockLivePlayerStore({ now: () => 1_000 });
  const context = {
    ...dstContext("username-binding", 1), module_id: "projectzomboid",
    player_list: { ...dstPlayerList, source: "runtime_action", response_codec: "zomboid_players", identity_kind: "player_name", player_action_ids: ["kick_user"] }
  };
  const ready = store.refresh(context);
  assert.deepEqual(ready.entries[0].identifiers, [{ kind: "player_name", value: "HostAlice", stable: false }]);
  assert.deepEqual(ready.entries[0].available_action_ids, ["kick_user"]);
  assert.equal(store.execute(context, {
    instance_id: context.instance_id, snapshot_id: ready.snapshot_id,
    player_key: ready.entries[0].player_key, action_id: "kick_user"
  }).status, "sent");

  for (const source of ["server_query", "console_log"]) {
    const readOnly = store.refresh({
      ...context, instance_id: `readonly-${source}`,
      player_list: { ...context.player_list, source }
    });
    assert.deepEqual(readOnly.entries[0].identifiers, []);
    assert.deepEqual(readOnly.entries[0].available_action_ids, []);
  }
});
