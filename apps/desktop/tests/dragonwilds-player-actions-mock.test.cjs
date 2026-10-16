const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, parent, isMain, options) {
  return typeof request === "string" && request.endsWith("?raw")
    ? `${resolve.call(this, request.slice(0, -4), parent, isMain, options)}?raw`
    : resolve.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (module, filename) => {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(filename.slice(0, -4), "utf8"))};`, filename);
};
const { MockLivePlayerStore, parseMockPlayerListFromModuleToml } = require(path.resolve(__dirname, "../src/api-mock/live-players.ts"));
const list = parseMockPlayerListFromModuleToml("runescapedragonwilds");
const context = () => ({ instance_id: "dragon-control", module_id: "runescapedragonwilds", running: true,
  player_list: structuredClone(list), current_players: 2, max_players: null, settings: {} });
function populated() { return new MockLivePlayerStore({ now: () => 1000 }).refresh(context()).entries; }
function withRows(rows) {
  return new MockLivePlayerStore({ now: () => 1000, collector: () => ({ status: "ready", entries: rows,
    complete: true, truncated: false, issue: null, current_players: rows.length }) });
}

test("Dragonwilds manifest preview binds EOS accounts and sessions to only current row actions", () => {
  assert.equal(list.identity_kind, "eos_id");
  assert.deepEqual(list.player_action_ids, ["kick_player", "ban_player"]);
  const snapshot = new MockLivePlayerStore({ now: () => 1000 }).refresh(context());
  assert.equal(snapshot.expires_at_unix_ms, 16_000);
  for (const row of snapshot.entries) {
    assert.deepEqual(row.available_action_ids, ["kick_player", "ban_player"]);
    assert.equal(row.identifiers[0].kind, "eos_id");
    assert.equal(row.identifiers[0].stable, true);
    assert.match(row.identifiers[0].value, /^[0-9a-f]{32}$/);
    assert.equal(row.identifiers[1].kind, "session_id");
    assert.equal(row.identifiers[1].stable, false);
  }
});
test("Dragonwilds query-only fallback and another game's file source do not gain control", () => {
  const old = context(); old.player_list.identity_kind = "session_id";
  assert.ok(new MockLivePlayerStore().refresh(old).entries.every((row) => row.available_action_ids.length === 0));
  const windrose = { ...context(), module_id: "windrose", player_list: parseMockPlayerListFromModuleToml("windrose") };
  assert.ok(new MockLivePlayerStore().refresh(windrose).entries.every((row) => row.available_action_ids.length === 0));
});
test("Dragonwilds row capabilities never expand a native subset or include unban", () => {
  const rows = populated(); rows[0].available_action_ids = ["kick_player", "unban_player"];
  const contract = context(); contract.player_list.player_action_ids.push("unban_player");
  const snapshot = withRows(rows).refresh(contract);
  assert.deepEqual(snapshot.entries[0].available_action_ids, ["kick_player"]);
  rows[0].available_action_ids = [];
  assert.deepEqual(withRows(rows).refresh(context()).entries[0].available_action_ids, []);
});
test("Dragonwilds missing, conflicting or unverified accounts cannot acquire row actions", () => {
  for (const alter of [
    (row) => { row.identifiers = [{ kind: "session_id", value: "1", stable: false }]; },
    (row) => { row.identifiers[0].stable = false; },
    (row) => { row.identifiers[0].kind = "steam_id"; },
    (row) => { row.identifiers[0].value = "76561198000000001"; },
    (row) => { row.identifiers[1].value = "01"; },
    (row) => { row.identifiers[1].value = "2147483648"; },
    (row) => { row.identifiers.push(structuredClone(row.identifiers[0])); },
  ]) {
    const rows = populated(); alter(rows[0]);
    assert.deepEqual(withRows(rows).refresh(context()).entries[0].available_action_ids, []);
  }
});
test("Dragonwilds duplicate accounts or sessions remove authority from every conflicting row", () => {
  for (const kind of ["eos_id", "session_id"]) {
    const rows = populated();
    rows[1].identifiers.find((identity) => identity.kind === kind).value = rows[0].identifiers.find((identity) => identity.kind === kind).value;
    assert.ok(withRows(rows).refresh(context()).entries.every((row) => row.available_action_ids.length === 0));
  }
});
test("Dragonwilds expired snapshots and reused action references cannot execute", () => {
  let now = 1000;
  const store = new MockLivePlayerStore({ now: () => now });
  const ctx = context(); const snapshot = store.refresh(ctx);
  const input = { instance_id: ctx.instance_id, snapshot_id: snapshot.snapshot_id,
    player_key: snapshot.entries[0].player_key, action_id: "kick_player" };
  now = 16_000;
  assert.throws(() => store.execute(ctx, input), /expired/);
  now = 16_001;
  const current = store.refresh(ctx);
  const fresh = { ...input, snapshot_id: current.snapshot_id, player_key: current.entries[0].player_key };
  store.execute(ctx, fresh);
  assert.throws(() => store.execute(ctx, fresh), /current/);
});
