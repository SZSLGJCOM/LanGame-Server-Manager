const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "../../..");
const sourceRoot = path.resolve(__dirname, "../src");
function load(relative) {
  const sourcePath = path.join(sourceRoot, relative);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), {
    module, exports: module.exports, URL,
    require(specifier) {
      if (specifier === "../../../domain/player-access") return load("domain/player-access.ts");
      return require(specifier);
    }
  }, { filename: sourcePath });
  return module.exports;
}
const { resolveSelectedRosterTarget: resolve, resolveSelectedRosterIdentity: resolveIdentity,
  isPlayerRosterCapability } = load("views/servers/player-center/player-access-selected-target.ts");
function field(moduleId, key) {
  const property = JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8")).properties[key];
  assert.ok(property, `${moduleId}.${key} must exist`);
  return { key, property };
}
function player(...identifiers) {
  return {
    player_key: "selected", display_name: "DisplayName", identifiers,
    available_action_ids: [], ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: []
  };
}
const id = (kind, value, stable = true) => ({ kind, value, stable });
const steamId = "76561198000000001";
const uuid = "12345678-1234-4234-8234-123456789abc";
const plain = (value) => JSON.parse(JSON.stringify(value));

test("Steam rosters choose the Steam namespace regardless of identifier order", () => {
  const target = field("squad", "admin_steam_ids");
  const identifiers = [id("eos_id", "deadbeef"), id("steam_id", steamId), id("session_id", "42", false)];
  assert.equal(resolve(player(...identifiers), target).rawValue, steamId);
  assert.equal(resolve(player(...identifiers.reverse()), target).rawValue, steamId);
  assert.equal(resolve(player(id("eos_id", steamId)), target), null);
  assert.equal(resolve(player(id("steam_id", steamId, false)), target), null);
});

test("missing, invalid, ambiguous and read-only targets never become quick actions", () => {
  const target = field("squad", "admin_steam_ids");
  assert.equal(resolve(null, target), null);
  assert.equal(resolve(player(), target), null);
  assert.equal(resolve(player(id("steam_id", "bad")), target), null);
  assert.equal(resolve(player(id("steam_id", steamId), id("steam_id", "76561198000000002")), target), null);
  assert.equal(resolve(player(id("steam_id", steamId)), { ...target, property: { ...target.property, readOnly: true } }), null);
});

test("Rust delimited entries and Klei IDs use their declared codec", () => {
  assert.equal(resolve(player(id("steam_id", steamId)), field("rust", "owner_entries")).rawValue, steamId);
  const target = field("dontstarve", "admin_list");
  assert.equal(resolve(player(id("steam_id", steamId), id("klei_user_id", "KU_Abcd")), target).rawValue, "KU_Abcd");
  assert.equal(resolve(player(id("steam_id", steamId)), target), null);
});

test("Minecraft requires both explicit UUID and account name, never display_name", () => {
  const target = field("minecraft", "whitelist_entries");
  assert.equal(resolve(player(id("minecraft_uuid", uuid)), target), null);
  assert.equal(resolve(player(id("player_name", "Alex", false)), target), null);
  assert.equal(resolve(player(id("minecraft_uuid", uuid), id("player_name", "Alex", false)), target).rawValue, `${uuid},Alex`);
  assert.equal(resolve(player(id("minecraft_uuid", uuid), id("player_name", "Alex,Other", false)), target), null);
});

test("plain name entries require explicit name identifiers and a known name contract", () => {
  const target = field("necesse", "owner_name");
  assert.equal(resolve(player(id("player_name", "Alex", false)), target).rawValue, "Alex");
  assert.equal(resolve(player(), target), null);
  assert.equal(resolve(player(id("steam_id", steamId)), target), null);
  assert.equal(resolve(player(id("player_name", "Alex", false)), {
    key: "arbitrary_names", property: { type: "array", "x-lsgm-player-access-codec": "plain" }
  }), null);
  assert.equal(resolve(player(id("player_name", "bad-name", false)), target), null);
});

test("object identities bind account and actual platform, not the default platform", () => {
  const target = field("sevendaystodie", "admin_users");
  assert.deepEqual(plain(resolve(player(id("steam_id", steamId)), target).rawValue), {
    platform: "Steam", userid: steamId, name: "", permission_level: 0
  });
  assert.equal(resolve(player(id("eos_id", "deadbeef")), target).rawValue.platform, "EOS");
  assert.equal(resolve(player(id("session_id", "42", false)), target), null);
  assert.equal(resolve(player(id("steam_id", steamId), id("eos_id", "deadbeef")), target), null);
});

test("Steam player accounts are never written into Steam group rosters", () => {
  for (const key of ["admin_groups", "whitelist_groups"]) {
    assert.equal(resolve(player(id("steam_id", steamId)), field("sevendaystodie", key)), null);
    assert.equal(resolveIdentity(player(id("steam_id", steamId)), field("sevendaystodie", key)), null);
    assert.equal(isPlayerRosterCapability(field("sevendaystodie", key)), false);
  }
});

test("trusted object identity can open a parameter form without inventing its required data", () => {
  const target = {
    key: "admins", property: {
      type: "array", "x-lsgm-player-access-codec": "object_identity",
      items: { type: "object", required: ["steam_id", "note"], properties: {
        steam_id: { type: "string", pattern: "^[0-9]{17}$", "x-lsgm-player-access-identity": true },
        note: { type: "string" }
      } }
    }
  };
  assert.equal(resolve(player(id("steam_id", steamId)), target), null, "A complete entry still needs its required note");
  assert.deepEqual(plain(resolveIdentity(player(id("steam_id", steamId)), target)), {
    identity: steamId, rawValue: { steam_id: steamId }
  });
  assert.equal(resolveIdentity(player(id("steam_id", steamId), id("steam_id", "76561198000000002")), target), null);
  assert.equal(resolveIdentity(player(id("session_id", "42", false)), target), null);
  assert.equal(resolveIdentity(player(id("steam_id", steamId)), { ...target, property: { ...target.property, readOnly: true } }), null);
});

test("object targets require every identity and required data without inventing defaults", () => {
  const target = {
    key: "players", property: {
      type: "array", "x-lsgm-player-access-codec": "object_identity",
      items: { type: "object", required: ["steam_id", "note"], properties: {
        steam_id: { type: "string", pattern: "^[0-9]{17}$", "x-lsgm-player-access-identity": true },
        note: { type: "string" }
      } }
    }
  };
  assert.equal(resolve(player(id("steam_id", steamId)), target), null);
  target.property.items.properties.note.default = "";
  assert.equal(resolve(player(id("steam_id", steamId)), target), null);
  target.property.items.properties.note.default = "Approved";
  assert.equal(resolve(player(id("steam_id", steamId)), target).rawValue.note, "Approved");
  target.property.items.properties.note["x-lsgm-player-access-identity"] = true;
  assert.equal(resolve(player(id("steam_id", steamId)), target), null);
});

test("ARK account rosters accept one proven account namespace and reject ambiguous accounts", () => {
  const target = field("arksurvivalascended", "exclusive_join_list");
  assert.equal(resolve(player(id("eos_id", "00000000000000000000000000000001")), target).rawValue, "00000000000000000000000000000001");
  assert.equal(resolve(player(id("steam_id", steamId)), target).rawValue, steamId);
  assert.equal(resolve(player(id("steam_id", steamId), id("eos_id", "different")), target), null);
  assert.equal(resolve(player(id("session_id", "123", false)), target), null);
});

test("Barotrauma account codec normalizes a proven Steam account without using a session ID", () => {
  const target = field("barotrauma", "admin_entries");
  assert.equal(resolve(player(id("steam_id", steamId)), target).rawValue, "STEAM_1:1:19867136");
});

test("a live session ID and unknown namespaces cannot fill platform or IP rosters", () => {
  assert.equal(resolve(player(id("session_id", "42", false)), field("barotrauma", "admin_entries")), null);
  assert.equal(resolve(player(id("steam_id", steamId)), field("minecraft", "banned_ip_entries")), null);
  assert.equal(resolve(player(id("steam_id", steamId)), field("valheim", "admin_list")), null);
});

test("Valheim character bans require an explicit native name and keep platform rosters separate", () => {
  const ban = field("valheim", "banned_list");
  assert.equal(ban.property["x-lsgm-player-access-codec"], "valheim_ban");
  const named = player(id("session_id", "123", false), id("player_name", "Raven 北境", false));
  assert.equal(resolve(named, ban).rawValue, "Raven 北境");
  assert.equal(resolve(named, ban).identity, "Raven 北境");
  for (const unsupported of [player(), player(id("session_id", "Raven", false)), player(id("steam_id", steamId)),
    player(id("player_name", "Raven", false), id("player_name", "Other", false)), player(id("player_name", "bad\nname", false))]) {
    assert.equal(resolve(unsupported, ban), null, "Labels, sessions, accounts and ambiguous names cannot become character bans");
  }
  for (const key of ["admin_list", "permitted_list"]) {
    assert.equal(resolve(named, field("valheim", key)), null);
    assert.equal(resolve(player(id("steam_id", steamId), id("player_name", "Raven", false)), field("valheim", key)), null);
  }
});

test("Valheim verified native platform identities drive account rosters and take precedence over character names", () => {
  for (const platformId of [`Steam_${steamId}`, "Xbox_NativeAccount_A7", "PlayFab_NativeAccount_B8"]) {
    const account = player(id("player_name", "Raven", false), id("steam_id", "76561198000000009"),
      id("valheim_platform_id", platformId));
    for (const key of ["admin_list", "permitted_list", "banned_list"]) {
      const target = resolve(account, field("valheim", key));
      assert.equal(target.rawValue, platformId, `${key} preserves the native platform ID exactly`);
      assert.equal(target.identity, platformId, `${key} preserves the native case-sensitive account identity`);
    }
  }
});

test("Valheim ambiguous or unstable native platform bindings cannot downgrade into a name ban", () => {
  for (const identifiers of [
    [id("valheim_platform_id", `Steam_${steamId}`), id("valheim_platform_id", "Xbox_Other")],
    [id("valheim_platform_id", "Xbox_NativeAccount", false)],
    [id("valheim_platform_id", "")],
    [id("valheim_platform_id", "Xbox_Not A Native ID")],
    [id("valheim_platform_id", "Xbox_Invalid\n")],
    [id("valheim_platform_id", "Xbox_NativeAccount"), id("valheim_platform_id", "xbox_nativeaccount")]
  ]) {
    const account = player(...identifiers, id("player_name", "Raven", false));
    for (const key of ["admin_list", "permitted_list", "banned_list"]) assert.equal(resolve(account, field("valheim", key)), null);
  }
  const repeated = player(id("valheim_platform_id", "Xbox_NativeAccount"), id("valheim_platform_id", "Xbox_NativeAccount"));
  assert.equal(resolve(repeated, field("valheim", "admin_list")).rawValue, "Xbox_NativeAccount");
});
