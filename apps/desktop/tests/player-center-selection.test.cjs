const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const src = path.resolve(__dirname, "../src");
function load(relative) {
  const filename = path.join(src, relative);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports, URL,
    require(id) {
      if (id === "../../../domain/player-access") return load("domain/player-access.ts");
      if (id === "../../../domain/live-player-state") return load("domain/live-player-state.ts");
      if (id === "./player-access-selected-target") return load("views/servers/player-center/player-access-selected-target.ts");
      if (id === "./player-access-roster-model") return { isRosterRecord: (value) => value !== null && typeof value === "object" && !Array.isArray(value) };
      return require(id);
    }
  }, { filename });
  return module.exports;
}
const domain = load("domain/player-access.ts");
const api = load("views/servers/player-center/player-center-selection.ts");
const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules/sevendaystodie/schema.json"), "utf8"));
const steamId = "76561198000000001";
function field(key, raw = []) {
  const property = schema.properties[key];
  return { key, title: key, kind: "object-list", lane: key === "blacklist_entries" ? "block" : "admin", property,
    currentValue: raw, entries: domain.decodePlayerAccessEntries(key, property, raw) };
}
function selection(key = "blacklist_entries", raw = { platform: "Steam", userid: steamId, name: "Alice", unbandate: "2036-09-27 12:34:56", reason: "Native" }) {
  const source = field(key, [raw]);
  assert.equal(source.entries.length, 1);
  return { field: source, entry: source.entries[0] };
}
function live(playerKey = "live-alice", value = steamId) {
  return { player_key: playerKey, display_name: "Alice", identifiers: [{ kind: "steam_id", value, stable: true }],
    available_action_ids: ["kick_player", "ban_player"], attributes: [], ping_ms: null, session_started_at_unix_ms: null, role: null };
}
function snapshot(entries) { return { status: "ready", complete: true, truncated: false, stale: false, expires_at_unix_ms: 200, entries }; }

test("offline roster account fills other declared personal rosters without runtime authority", () => {
  const selected = selection();
  const account = api.rosterEntryPlayer(selected);
  assert.deepEqual(Array.from(account.available_action_ids), []);
  for (const key of ["admin_users", "whitelist_users"]) {
    const target = api.resolveRosterActionTarget(account, selected, field(key));
    assert.equal(target.rawValue.platform, "Steam");
    assert.equal(target.rawValue.userid, steamId);
  }
  assert.equal(api.resolveRosterActionTarget(account, selected, selected.field).rawValue, selected.entry.rawValue);
});

test("EOS roster identities retain their actual namespace instead of the schema default", () => {
  const selected = selection("blacklist_entries", { platform: "EOS", userid: "deadbeef", unbandate: "9999-12-31", reason: "Native" });
  const account = api.rosterEntryPlayer(selected);
  const target = api.resolveRosterActionTarget(account, selected, field("admin_users"));
  assert.equal(target.rawValue.platform, "EOS");
  assert.equal(target.rawValue.userid, "deadbeef");
});

test("runtime selection returns only the unique real fresh snapshot row", () => {
  const row = live();
  assert.ok(Object.is(api.matchRosterLivePlayer(selection(), snapshot([row]), 100), row));
  assert.equal(api.matchRosterLivePlayer(selection(), snapshot([row, live("duplicate")]), 100), null);
  assert.equal(api.matchRosterLivePlayer(selection(), snapshot([live("other", "76561198000000009")]), 100), null);
});

test("stale incomplete expired failed and missing snapshots never authorize roster runtime actions", () => {
  const selected = selection();
  for (const patch of [{ status: "stopped" }, { status: "failed" }, { status: "refreshing" }, { stale: true },
    { complete: false }, { truncated: true }, { expires_at_unix_ms: 100 }, { expires_at_unix_ms: null }, { expires_at_unix_ms: Number.NaN }, { expires_at_unix_ms: Infinity }]) {
    assert.equal(api.matchRosterLivePlayer(selected, { ...snapshot([live()]), ...patch }, 100), null, JSON.stringify(patch));
  }
  assert.equal(api.matchRosterLivePlayer(selected, null, 100), null);
});

test("group roster entries cannot become personal accounts or acquire runtime authorization", () => {
  const selected = selection("admin_groups", { steam_id: "103582791429521412", permission_level: 1000 });
  assert.equal(api.rosterEntryPlayer(selected), null);
  assert.equal(api.resolveRosterActionTarget(null, selected, field("admin_users")), null);
  assert.equal(api.matchRosterLivePlayer(selected, snapshot([live()]), 100), null);
  assert.equal(api.resolveRosterActionTarget(null, selected, selected.field).rawValue, selected.entry.rawValue);
});

test("invalid raw identities cannot be promoted by a plausible entry key", () => {
  const selected = selection();
  assert.equal(api.rosterEntryPlayer({ ...selected, entry: { ...selected.entry, rawValue: { platform: "Steam", userid: "42" } } }), null);
  assert.equal(api.rosterEntryPlayer({ ...selected, entry: { ...selected.entry, key: "unrelated" } }), null);
});

test("unrelated plain-text rosters cannot invent a shared account namespace", () => {
  const property = { type: "array", items: { type: "string" }, "x-lsgm-player-access-codec": "plain" };
  const source = { key: "custom_source", property };
  const target = { key: "custom_target", property };
  const selected = { field: source, entry: { key: "alice", label: "Alice", rawValue: "Alice" } };
  assert.equal(api.rosterEntryPlayer(selected), null);
  assert.equal(api.resolveRosterActionTarget(null, selected, target), null);
  assert.equal(api.resolveRosterActionTarget(null, selected, source).rawValue, "Alice");
});
