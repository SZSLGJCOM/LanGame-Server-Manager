const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");

function loadPlayerAccessActions() {
  const sourcePath = path.join(
    __dirname,
    "..",
    "src",
    "domain",
    "player-access.ts"
  );
  const source = fs.readFileSync(sourcePath, "utf8");
  const outputText = transpileTypeScript(source, sourcePath);

  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require,
    URL
  }, { filename: sourcePath });
  return module.exports;
}

function moduleProperty(moduleId, fieldKey) {
  const schemaPath = path.join(repositoryRoot, "modules", moduleId, "schema.json");
  const schema = JSON.parse(fs.readFileSync(schemaPath, "utf8"));
  const property = schema.properties?.[fieldKey];
  assert.ok(property, `${moduleId}.${fieldKey} must exist`);
  return property;
}

function moduleToml(moduleId) {
  return fs.readFileSync(path.join(repositoryRoot, "modules", moduleId, "module.toml"), "utf8");
}

function realModuleActions(moduleId) {
  const ids = Array.from(
    moduleToml(moduleId).matchAll(/\[\[runtime\.player_actions\]\][\s\S]*?^id\s*=\s*"([^"]+)"/gm),
    (match) => match[1]
  );
  return actions(...ids);
}

function actions(...ids) {
  return ids.map((id) => ({ id }));
}

function onlyBinding(api, moduleId, fieldKey, runtimeActions) {
  const property = moduleProperty(moduleId, fieldKey);
  const bindings = api.buildPlayerAccessBindings(
    [{ key: fieldKey, property }],
    runtimeActions
  );
  assert.equal(bindings.length, 1, `${moduleId}.${fieldKey} must produce one exact binding`);
  return bindings[0];
}

test("fields without both explicit player-access extensions never bind or consume actions", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = [
    { id: "ban_player", label: "Ban" },
    { id: "unrelated", label: "封禁玩家" }
  ];

  const noMetadata = api.buildPlayerAccessBindings([
    {
      key: "looks_like_a_ban_list",
      property: {
        type: "string",
        title: "Ban list",
        "x-lsgm-player-access-kind": "block"
      }
    }
  ], runtimeActions);
  assert.equal(noMetadata.length, 0);

  const codecOnly = api.buildPlayerAccessBindings([
    {
      key: "still_not_declared",
      property: { type: "string", "x-lsgm-player-access-codec": "steam64" }
    }
  ], runtimeActions);
  assert.equal(codecOnly.length, 0);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, noMetadata), (action) => action.id),
    ["ban_player", "unrelated"]
  );
  assert.equal(api.parsePlayerAccessSync({
    "x-lsgm-player-access-sync": { mode: "direct" }
  }), null);
});

test("one-sided direct synchronization applies only to the declared operation", () => {
  const api = loadPlayerAccessActions();
  for (const operation of ["add", "remove"]) {
    const sync = api.parsePlayerAccessSync({
      "x-lsgm-player-access-sync": { mode: "direct", [`${operation}_action_id`]: "roster_action" }
    });
    assert.ok(sync);
    assert.equal(api.playerAccessMutationActionId(sync, operation), "roster_action");
    assert.equal(api.playerAccessMutationActionId(sync, operation === "add" ? "remove" : "add"), null);
  }
});

test("runtime player-action token guard rejects control and command syntax fragments", () => {
  const api = loadPlayerAccessActions();

  for (const invalid of [
    `Alice${String.fromCharCode(0)}`,
    "Alice\nBob",
    `Alice${String.fromCharCode(0x85)}`,
    "{{target}}",
    "Alice}}",
    "Alice;ban Bob",
    "Alice&&ban Bob",
    "Alice || ban Bob",
    "`whoami`",
    "$(whoami)"
  ]) {
    assert.equal(
      api.hasUnsupportedRuntimeActionSyntax(invalid),
      true,
      `unsafe runtime action token was accepted: ${JSON.stringify(invalid)}`
    );
  }

  for (const valid of ["Alice", "Alice Bob", "KU_demo-admin", "A&B", "$player", "(Alice)"]) {
    assert.equal(
      api.hasUnsupportedRuntimeActionSyntax(valid),
      false,
      `safe runtime action token was rejected: ${JSON.stringify(valid)}`
    );
  }
});

test("7DTD keeps full blacklist metadata and consumes only its declared live removal action", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("list_players", "ban_player", "unban_player", "list_bans");
  const rawEntry = {
    platform: "Steam",
    userid: "76561198000000001",
    name: "Alice",
    unbandate: "9999-12-31",
    reason: "LanGame"
  };
  const binding = onlyBinding(
    api,
    "sevendaystodie",
    "blacklist_entries",
    runtimeActions
  );
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  const entries = api.decodePlayerAccessEntries(
    "blacklist_entries",
    property,
    [rawEntry, { ...rawEntry, name: "duplicate" }]
  );

  assert.equal(api.parsePlayerAccessCodec(property), "object_identity");
  assert.equal(property.items.properties.unbandate.type, "string");
  assert.equal(property.items.properties.unbandate.default, "9999-12-31");
  assert.equal(binding.sync.mode, "direct");
  assert.equal(binding.sync.addActionId, null);
  assert.equal(binding.sync.removeActionId, "unban_player");
  assert.equal(binding.sync.verifyActionId, null);
  assert.equal(api.playerAccessMutationActionId(binding.sync, "add"), null);
  assert.equal(api.playerAccessMutationActionId(binding.sync, "remove"), "unban_player");
  assert.equal(entries.length, 1);
  assert.equal(entries[0].key, 'platform="steam"|userid="76561198000000001"');
  assert.equal(entries[0].label, "Steam / 76561198000000001 / Alice");
  assert.deepEqual(entries[0].rawValue, rawEntry);
  assert.equal(
    api.canonicalPlayerAccessIdentity("object_identity", rawEntry, property),
    'platform="steam"|userid="76561198000000001"'
  );
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), ["unban_player"]);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["list_players", "ban_player", "list_bans"]
  );
});

test("7DTD live removal targets require an explicit valid platform and user ID", () => {
  const api = loadPlayerAccessActions();
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  for (const operation of ["add", "remove"]) {
    assert.equal(api.playerAccessLiveTarget(property, { platform: "Steam", userid: "76561198000000001" }, operation), "Steam_76561198000000001");
    assert.equal(api.playerAccessLiveTarget(property, { platform: "EOS", userid: "0002ab34ef" }, operation), "EOS_0002ab34ef");
    for (const invalid of [
      { userid: "76561198000000001" }, { platform: "", userid: "76561198000000001" },
      { platform: "unknown", userid: "76561198000000001" }, { platform: "Steam", userid: "bad;command" },
      { platform: "Steam", userid: "bad\ncommand" }, { platform: "Steam" },
      { platform: "Steam", userid: "42" }, { platform: "Steam", userid: "abcdefghijklmnopq" },
      { platform: "EOS", userid: "dead" }, { platform: "EOS", userid: "z".repeat(32) },
      { platform: "EOS", userid: "a".repeat(33) }
    ]) assert.equal(api.playerAccessLiveTarget(property, invalid, operation), null, JSON.stringify(invalid));
  }
  for (const userid of ["deadbeef", "a".repeat(32)]) {
    assert.equal(api.playerAccessLiveTarget(property, { platform: "EOS", userid }, "remove"), `EOS_${userid}`);
  }
});

test("object rosters preserve full raw values and compose every declared identity field", () => {
  const api = loadPlayerAccessActions();
  const property = {
    type: "array",
    items: {
      type: "object",
      properties: {
        platform: {
          type: "string",
          enum: ["Steam", "EOS"],
          default: "Steam",
          "x-lsgm-player-access-identity": true
        },
        userid: {
          type: "string",
          pattern: "^[0-9]{17}$",
          "x-lsgm-player-access-identity": true
        },
        name: { type: "string" },
        permission_level: { type: "integer", default: 0 },
        unbandate: { type: "string", format: "date", default: "9999-12-31" },
        reason: { type: "string", default: "LanGame" }
      }
    },
    "x-lsgm-player-access-codec": "object_identity"
  };
  const rawEntry = {
    platform: "Steam",
    userid: "76561198000000001",
    name: "Alice",
    permission_level: 0,
    unbandate: "9999-12-31",
    reason: "LanGame"
  };
  const duplicateIdentity = { ...rawEntry, name: "Updated Alice", reason: "Updated" };
  const entries = api.decodePlayerAccessEntries("users", property, [rawEntry, duplicateIdentity]);

  assert.deepEqual(Array.from(api.playerAccessObjectIdentityKeys(property)), ["platform", "userid"]);
  assert.equal(
    api.canonicalPlayerAccessIdentity("object_identity", rawEntry, property),
    'platform="steam"|userid="76561198000000001"'
  );
  assert.equal(entries.length, 1);
  assert.equal(entries[0].label, "Steam / 76561198000000001 / Alice");
  assert.deepEqual(entries[0].rawValue, rawEntry);
  assert.equal(api.canonicalPlayerAccessAdditionIdentity("object_identity", rawEntry, property), entries[0].key);
  assert.equal(api.canonicalPlayerAccessAdditionIdentity(
    "object_identity",
    { ...rawEntry, platform: "Unknown" },
    property
  ), null);
  assert.equal(api.canonicalPlayerAccessIdentity(
    "object_identity",
    { ...rawEntry, userid: "invalid" },
    property
  ), null);
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity(
      "object_identity",
      { ...rawEntry, name: `Alice${String.fromCharCode(0x85)}` },
      property
    ),
    null
  );
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity(
      "object_identity",
      { ...rawEntry, name: "  Alice  ", reason: "  Trusted  " },
      property
    ),
    entries[0].key
  );
});

test("7DTD native blacklist expiry accepts real dates and local seconds without truncation", () => {
  const api = loadPlayerAccessActions();
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  assert.equal(property.items.properties.unbandate.format, "date-or-local-datetime");
  const base = { platform: "Steam", userid: "76561198000000001", name: "Alice", reason: "LanGame" };
  for (const unbandate of ["9999-12-31", "2036-02-29", "2036-02-29 00:00:00", "2037-04-30 23:59:59"]) {
    const entry = { ...base, unbandate };
    assert.notEqual(api.canonicalPlayerAccessAdditionIdentity("object_identity", entry, property), null, unbandate);
    const entries = api.decodePlayerAccessEntries("blacklist_entries", property, [entry]);
    assert.equal(entries.length, 1, unbandate);
    assert.equal(entries[0].rawValue.unbandate, unbandate);
  }
  for (const unbandate of ["2035-02-29", "1900-02-29", "2036-02-30", "2037-04-31", "2037-13-01", "2037-01-00",
    "2037-04-30 24:00:00", "2037-04-30 23:60:00", "2037-04-30 23:59:60", "2037-04-30 1:02:03",
    "2037-04-30T23:59:59Z", "2037-04-30 23:59:59 extra"]) {
    assert.equal(api.canonicalPlayerAccessAdditionIdentity("object_identity", { ...base, unbandate }, property), null, unbandate);
  }
});

test("plain roster follows real schema patterns and scalar Steam64 stays single-valued", () => {
  const api = loadPlayerAccessActions();
  const owner = moduleProperty("necesse", "owner_name");
  const unturnedOwner = moduleProperty("unturned", "owner_steam_id");

  assert.equal(api.canonicalPlayerAccessAdditionIdentity("plain", "Alice_1", owner), "alice_1");
  assert.equal(api.canonicalPlayerAccessAdditionIdentity("plain", "Alice-1", owner), null);
  assert.equal(api.canonicalPlayerAccessAdditionIdentity("plain", 'Alice"1', owner), null);
  assert.deepEqual(
    Array.from(api.decodePlayerAccessEntries(
      "owner_steam_id",
      unturnedOwner,
      "76561198000000001\n76561198000000002"
    )),
    []
  );
});

test("renderer-specific roster codecs mirror accepted identity boundaries", () => {
  const api = loadPlayerAccessActions();
  const property = (codec) => ({ "x-lsgm-player-access-codec": codec });

  assert.equal(api.canonicalPlayerAccessIdentity("ark_account_id", "EOS:Account_42", property("ark_account_id")), "eos:account_42");
  for (const invalid of ["", "account name", "account,42", "account|42", "account;42", "{{account}}", "x".repeat(129), "账户"] ) {
    assert.equal(api.canonicalPlayerAccessIdentity("ark_account_id", invalid, property("ark_account_id")), null);
  }

  const steam64 = "76561198000000001";
  const accountId = BigInt(steam64) - 76561197960265728n;
  const steam2 = `STEAM_1:${accountId % 2n}:${accountId / 2n}`;
  const barotraumaProperty = moduleProperty("barotrauma", "admin_entries");
  assert.equal(api.canonicalPlayerAccessIdentity("barotrauma_account", `${steam64},Alice`, barotraumaProperty), steam2.toLowerCase());
  assert.equal(api.canonicalPlayerAccessIdentity("barotrauma_account", steam2, barotraumaProperty), steam2.toLowerCase());
  assert.equal(api.canonicalPlayerAccessIdentity("barotrauma_account", `[U:1:${accountId}]`, barotraumaProperty), steam2.toLowerCase());
  for (const invalid of ["76561197960265728", "STEAM_1:2:42", "[U:2:42]", "Alice"]) {
    assert.equal(api.canonicalPlayerAccessIdentity("barotrauma_account", invalid, barotraumaProperty), null);
  }

  assert.equal(api.canonicalPlayerAccessIdentity("dst_klei_id", "ku_demo-admin", property("dst_klei_id")), "ku_demo-admin");
  for (const invalid of ["KU_", "KU_A B", "KU_A,B", "KU_{A}", `KU_${"a".repeat(62)}`]) {
    assert.equal(api.canonicalPlayerAccessIdentity("dst_klei_id", invalid, property("dst_klei_id")), null);
  }

  assert.equal(api.canonicalPlayerAccessIdentity("valheim_platform_id", "Steam_76561198000000001", property("valheim_platform_id")), "steam_76561198000000001");
  for (const invalid of ["", "Steam 42", "Steam,42", "Steam|42", "Steam\n42"]) {
    assert.equal(api.canonicalPlayerAccessIdentity("valheim_platform_id", invalid, property("valheim_platform_id")), null);
  }
});

test("renderer-specific roster modules declare the same explicit codec and sync contract", () => {
  const api = loadPlayerAccessActions();
  const expectedFields = [
    ["arksurvivalascended", "admin_account_ids", "ark_account_id"],
    ["arksurvivalascended", "exclusive_join_list", "ark_account_id"],
    ["arksurvivalascended", "priority_join_list", "ark_account_id"],
    ["barotrauma", "admin_entries", "barotrauma_account"],
    ["dontstarve", "admin_list", "dst_klei_id"],
    ["dontstarve", "whitelist", "dst_klei_id"],
    ["dontstarve", "blocklist", "dst_klei_id"],
    ["valheim", "admin_list", "valheim_platform_id"],
    ["valheim", "banned_list", "valheim_platform_id"],
    ["valheim", "permitted_list", "valheim_platform_id"]
  ];

  for (const [moduleId, fieldKey, codec] of expectedFields) {
    const property = moduleProperty(moduleId, fieldKey);
    assert.equal(api.parsePlayerAccessCodec(property), codec, `${moduleId}.${fieldKey} codec`);
    assert.equal(api.parsePlayerAccessSync(property)?.mode, "restart", `${moduleId}.${fieldKey} sync mode`);
  }
});

test("HumanitZ roster uses complete NetIDs for direct actions and retains existing Steam64 entries", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("list_bans", "ban_player", "unban_player");
  const binding = onlyBinding(
    api,
    "humanitz",
    "banned_player_steam_ids",
    runtimeActions
  );
  const property = moduleProperty("humanitz", "banned_player_steam_ids");
  const fullNetId = `${"A".repeat(32)}|${"b".repeat(32)}`;
  const productNetId = `|${"C".repeat(32)}`;
  const retainedSteamId = "76561198000000002";
  const entries = api.decodePlayerAccessEntries(
    "banned_player_steam_ids",
    property,
    [fullNetId, productNetId, retainedSteamId, "PlayerName", fullNetId].join("\n")
  );

  assert.equal(api.parsePlayerAccessCodec(property), "humanitz_net_id");
  assert.deepEqual(Array.from(entries, (entry) => entry.key), [fullNetId, productNetId, retainedSteamId]);
  assert.equal(binding.sync.mode, "direct");
  assert.equal(binding.sync.addActionId, "ban_player");
  assert.equal(binding.sync.removeActionId, "unban_player");
  for (const value of [fullNetId, productNetId]) {
    assert.equal(api.canonicalPlayerAccessAdditionIdentity("humanitz_net_id", value, property), value);
  }
  for (const value of [retainedSteamId, "PlayerName", "b".repeat(32)]) {
    assert.equal(api.canonicalPlayerAccessAdditionIdentity("humanitz_net_id", value, property), null);
  }
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["list_bans"]
  );
});

test("ARK Survival Evolved priority roster folds exact no-check join actions", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = realModuleActions("arksurvivalevolved");
  const property = moduleProperty("arksurvivalevolved", "priority_join_list");
  const binding = onlyBinding(api, "arksurvivalevolved", "priority_join_list", runtimeActions);
  const entries = api.decodePlayerAccessEntries(
    "priority_join_list",
    property,
    "76561198000000005\n76561198000000005\nPlayerName"
  );

  assert.equal(api.parsePlayerAccessCodec(property), "steam64");
  assert.equal(binding.sync.mode, "direct");
  assert.equal(binding.sync.addActionId, "allow_no_check");
  assert.equal(binding.sync.removeActionId, "disallow_no_check");
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), ["allow_no_check", "disallow_no_check"]);
  assert.equal(entries.length, 1);
  assert.equal(entries[0].key, "76561198000000005");
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["broadcast", "list_players", "kick_player", "ban_player", "unban_player"]
  );

  const descriptor = moduleToml("arksurvivalevolved");
  assert.match(descriptor, /id = "allow_no_check"[\s\S]*?command_template = "AllowPlayerToJoinNoCheck \{\{target\}\}"/);
  assert.match(descriptor, /id = "disallow_no_check"[\s\S]*?command_template = "DisallowPlayerToJoinNoCheck \{\{target\}\}"/);
});

test("Unturned Steam64 admin roster sync stays narrower than its live ID-or-name actions", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = realModuleActions("unturned");
  const property = moduleProperty("unturned", "admin_steam_ids");
  const binding = onlyBinding(api, "unturned", "admin_steam_ids", runtimeActions);

  assert.equal(api.parsePlayerAccessCodec(property), "steam64");
  assert.equal(binding.sync.mode, "direct");
  assert.equal(binding.sync.addActionId, "add_admin");
  assert.equal(binding.sync.removeActionId, "remove_admin");
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), []);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    Array.from(runtimeActions, (action) => action.id)
  );

  const descriptor = moduleToml("unturned");
  assert.match(descriptor, /id = "add_admin"[\s\S]*?label = "Add admin immediately by ID or name"/);
  assert.match(descriptor, /id = "remove_admin"[\s\S]*?label = "Remove admin immediately by ID or name"/);
});

test("RimWorld retired whitelist has no binding or obsolete native actions", () => {
  const api = loadPlayerAccessActions();
  const schema = JSON.parse(fs.readFileSync(path.join(repositoryRoot, "modules", "rimworld", "schema.json"), "utf8"));
  assert.equal(schema.properties.use_whitelist, undefined);
  assert.equal(schema.properties.whitelisted_users, undefined);
  const fields = Object.entries(schema.properties).map(([key, property]) => ({ key, property }));
  const runtimeActions = realModuleActions("rimworld");
  assert.deepEqual(Array.from(api.buildPlayerAccessBindings(fields, runtimeActions)), []);
  assert.deepEqual(runtimeActions.map((action) => action.id), [
    "list_players", "list_bans", "kick_user", "ban_user", "unban_user", "op_player", "deop_player"
  ]);
  assert.doesNotMatch(moduleToml("rimworld"), /command_template = "whitelist/);
});

test("Minecraft CSV roster stays persistent-only until UUID and username are authoritatively matched", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("list_players", "ban_player", "unban_player");
  const rawEntry = "A0B1C2D3-E4F5-4678-9ABC-DEF012345678,Steve,Griefing";
  const binding = onlyBinding(
    api,
    "minecraft",
    "banned_player_entries",
    runtimeActions
  );
  const property = moduleProperty("minecraft", "banned_player_entries");
  const entries = api.decodePlayerAccessEntries(
    "banned_player_entries",
    property,
    `${rawEntry}\nSteveOnly`
  );

  assert.equal(api.parsePlayerAccessCodec(property), "csv_uuid_name");
  assert.equal(binding.sync.mode, "restart");
  assert.equal(binding.sync.addActionId, null);
  assert.equal(binding.sync.removeActionId, null);
  assert.equal(entries.length, 1);
  assert.equal(entries[0].key, "a0b1c2d3-e4f5-4678-9abc-def012345678");
  assert.equal(entries[0].label, "a0b1c2d3-e4f5-4678-9abc-def012345678 / Steve");
  assert.equal(
    api.canonicalPlayerAccessIdentity("csv_uuid_name", "A0B1C2D3E4F546789ABCDEF012345678,Steve", property),
    "a0b1c2d3-e4f5-4678-9abc-def012345678"
  );
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity("csv_uuid_name", "A0B1C2D3E4F546789ABCDEF012345678,Player_1", property),
    "a0b1c2d3-e4f5-4678-9abc-def012345678"
  );
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity("csv_uuid_name", "A0B1C2D3-E4F5-4678-9ABC-DEF012345678,abcdefghijklmnop", property),
    "a0b1c2d3-e4f5-4678-9abc-def012345678"
  );
  for (const invalid of [
    "A0B1C2D3E4F546789ABCDEF012345678,Player-One",
    "A0B1C2D3E4F546789ABCDEF012345678,abcdefghijklmnopq",
    "A0B1C2D3E4F546789ABCDEF012345678,玩家",
    "A0B1C2D3E4F546789ABCDEF012345678"
  ]) {
    assert.equal(
      api.canonicalPlayerAccessAdditionIdentity("csv_uuid_name", invalid, property),
      null,
      `invalid Minecraft roster addition was accepted: ${invalid}`
    );
  }
  assert.deepEqual(
    Array.from(api.decodePlayerAccessEntries(
      "banned_player_entries",
      property,
      "A0B1C2D3E4F546789ABCDEF012345678,Player-One"
    )),
    []
  );
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), []);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["list_players", "ban_player", "unban_player"]
  );
});

test("Minecraft IP roster accepts canonical addresses and rejects malformed boundaries", () => {
  const api = loadPlayerAccessActions();
  const property = moduleProperty("minecraft", "banned_ip_entries");

  assert.equal(api.parsePlayerAccessCodec(property), "minecraft_ip_csv");
  assert.equal(
    api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "192.0.2.42,Griefing", property),
    "192.0.2.42"
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "2001:db8::42,Griefing", property),
    "2001:db8::42"
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity(
      "minecraft_ip_csv",
      "2001:0db8:0000:0000:0000:0000:0000:0042,Griefing",
      property
    ),
    "2001:db8::42"
  );
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "256.0.2.42", property), null);
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "192.000.2.42", property), null);
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "2001:db8:42", property), null);
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "2001::db8::42", property), null);
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "2001:db8::42:", property), null);
  assert.equal(api.canonicalPlayerAccessIdentity("minecraft_ip_csv", "1.2.3.4::", property), null);
  assert.equal(
    api.normalizePlayerAccessDelimitedEntry(
      "minecraft_ip_csv",
      "192.0.2.42,Repeated, comma reason",
      property,
      "add"
    ).stored,
    "192.0.2.42,Repeated, comma reason"
  );
});

test("delimited roster contracts reject fallback metadata and preserve final free text", () => {
  const api = loadPlayerAccessActions();
  const uuid = "A0B1C2D3E4F546789ABCDEF012345678";
  const operator = moduleProperty("minecraft", "operator_entries");
  const whitelist = moduleProperty("minecraft", "whitelist_entries");
  const bannedPlayer = moduleProperty("minecraft", "banned_player_entries");
  const rustBan = moduleProperty("rust", "banned_entries");
  const barotrauma = moduleProperty("barotrauma", "admin_entries");

  for (const invalid of [
    `${uuid},Steve,9,true`,
    `${uuid},Steve,4,maybe`,
    `${uuid},Steve,4,true,ignored`
  ]) {
    assert.equal(api.canonicalPlayerAccessAdditionIdentity("csv_uuid_name", invalid, operator), null);
  }
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity("csv_uuid_name", `${uuid},Steve,ignored`, whitelist),
    null
  );
  assert.equal(
    api.normalizePlayerAccessDelimitedEntry(
      "csv_uuid_name",
      `${uuid},Steve,Reason, with comma`,
      bannedPlayer,
      "add"
    ).stored,
    "a0b1c2d3-e4f5-4678-9abc-def012345678,Steve,Reason, with comma"
  );
  assert.equal(
    api.normalizePlayerAccessDelimitedEntry(
      "pipe_steam64",
      "76561198000000003|Reason|with pipe",
      rustBan,
      "add"
    ).stored,
    "76561198000000003|Reason|with pipe"
  );
  assert.equal(
    api.normalizePlayerAccessDelimitedEntry(
      "barotrauma_account",
      "76561198000000001,Captain, One",
      barotrauma,
      "add"
    ).parts[1],
    "Captain, One"
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity(
      "pipe_steam64",
      `76561198000000003|unsafe${String.fromCharCode(0x85)}`,
      rustBan
    ),
    null
  );
});

test("comma-aware Steam64 rosters decode legacy values once and plain values reject controls", () => {
  const api = loadPlayerAccessActions();
  const property = moduleProperty("vrising", "ban_list");
  const entries = api.decodePlayerAccessEntries(
    "ban_list",
    property,
    "76561198000000001, 76561198000000002\n76561198000000001"
  );

  assert.deepEqual(Array.from(entries, (entry) => entry.key), [
    "76561198000000001",
    "76561198000000002"
  ]);
  assert.equal(
    api.canonicalPlayerAccessAdditionIdentity(
      "plain",
      `Player${String.fromCharCode(0x85)}`,
      { "x-lsgm-player-access-codec": "plain" }
    ),
    null
  );
});

test("Rust pipe roster projects only Steam64 and leaves verification action visible", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("ban_steamid", "unban_steamid", "banlistex", "writecfg");
  const rawEntry = "76561198000000003|Alice|LanGame";
  const binding = onlyBinding(api, "rust", "banned_entries", runtimeActions);
  const property = moduleProperty("rust", "banned_entries");
  const entries = api.decodePlayerAccessEntries("banned_entries", property, rawEntry);

  assert.equal(api.parsePlayerAccessCodec(property), "pipe_steam64");
  assert.equal(entries[0].key, "76561198000000003");
  assert.equal(entries[0].label, rawEntry);
  assert.equal(binding.sync.verifyActionId, "banlistex");
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["banlistex", "writecfg"]
  );
});

test("V Rising keeps player access restart-backed because official RCON omits admin-console moderation commands", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("list_users", "list_bans", "ban_user", "unban_user", "reload_banlist");
  const rawEntry = "76561198000000004";
  const binding = onlyBinding(api, "vrising", "ban_list", runtimeActions);

  assert.equal(binding.sync.mode, "restart");
  assert.equal(binding.sync.addActionId, null);
  assert.equal(binding.sync.removeActionId, null);
  assert.equal(binding.sync.actionId, null);
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), []);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["list_users", "list_bans", "ban_user", "unban_user", "reload_banlist"]
  );
  const descriptor = moduleToml("vrising");
  assert.match(descriptor, /id = "broadcast"[\s\S]*?command_template = "announce \{\{target\}\}"/);
  assert.doesNotMatch(descriptor, /id = "(?:list_users|kick_user|ban_user|unban_user|list_bans|reload_banlist)"/);
  assert.match(descriptor, /status = "persistent_roster"/);
});

test("Terraria restart roster does not equate banlist entries with the live character command", () => {
  const api = loadPlayerAccessActions();
  const runtimeActions = actions("list_players", "ban_player");
  const rawEntry = "192.0.2.42";
  const binding = onlyBinding(api, "terraria", "banlist_entries", runtimeActions);

  assert.equal(api.parsePlayerAccessCodec(moduleProperty("terraria", "banlist_entries")), "terraria_banlist");
  assert.equal(binding.sync.mode, "restart");
  assert.deepEqual(Array.from(binding.consumedRuntimeActionIds), []);
  assert.deepEqual(
    Array.from(api.filterConsumedPlayerAccessActions(runtimeActions, [binding]), (action) => action.id),
    ["list_players", "ban_player"]
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", "192.0.2.42", moduleProperty("terraria", "banlist_entries")),
    "192.0.2.42"
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", "{{dangerous_template}}", moduleProperty("terraria", "banlist_entries")),
    null
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", `name${String.fromCharCode(0)}`, moduleProperty("terraria", "banlist_entries")),
    null
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", "x".repeat(129), moduleProperty("terraria", "banlist_entries")),
    null
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", "你".repeat(42), moduleProperty("terraria", "banlist_entries")),
    "你".repeat(42)
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", "你".repeat(43), moduleProperty("terraria", "banlist_entries")),
    null
  );
  assert.equal(
    api.canonicalPlayerAccessIdentity("terraria_banlist", `name${String.fromCharCode(0x85)}`, moduleProperty("terraria", "banlist_entries")),
    null
  );
});

test("player-access feedback exposes severity and roster entries expose list semantics", () => {
  const workbench = fs.readFileSync(
    path.join(repositoryRoot, "apps", "desktop", "src", "views", "servers", "player-center", "use-player-access.tsx"),
    "utf8"
  );
  const rosterList = fs.readFileSync(
    path.join(repositoryRoot, "apps", "desktop", "src", "views", "servers", "player-center", "PlayerAccessRosterList.tsx"),
    "utf8"
  );
  const activityNotice = fs.readFileSync(
    path.join(repositoryRoot, "apps", "desktop", "src", "components", "ActivityNotice.tsx"), "utf8");
  const styles = fs.readFileSync(
    path.join(repositoryRoot, "apps", "desktop", "src", "styles", "activity-bar.css"), "utf8");

  assert.match(workbench, /type PlayerAccessFeedbackTone = "neutral" \| "success" \| "warning" \| "error"/);
  assert.match(workbench, /<ActivityNotice tone=\{feedback\.tone === "neutral" \? "info" : feedback\.tone\}/);
  assert.match(activityNotice, /role=\{tone === "error" \? "alert" : "status"\}/);
  assert.match(rosterList, /className="player-access-roster-entries"\s+role="list"/);
  assert.match(rosterList, /className=\{`player-access-roster-entry[\s\S]*?role="listitem"/);
  for (const tone of ["success", "warning", "error"]) {
    assert.match(styles, new RegExp(`\\.shell-activity-notice\\.is-${tone}`));
  }
});

test("all declared player-access runtime action ids exist in the real module descriptors", () => {
  const api = loadPlayerAccessActions();
  for (const moduleId of [
    "arksurvivalevolved",
    "humanitz",
    "minecraft",
    "rimworld",
    "rust",
    "sevendaystodie",
    "terraria",
    "unturned",
    "vrising"
  ]) {
    const schemaPath = path.join(repositoryRoot, "modules", moduleId, "schema.json");
    const schema = JSON.parse(fs.readFileSync(schemaPath, "utf8"));
    const realActions = realModuleActions(moduleId);
    const actionIds = new Set(realActions.map((action) => action.id));

    for (const [fieldKey, property] of Object.entries(schema.properties ?? {})) {
      const sync = api.parsePlayerAccessSync(property);
      if (!sync) {
        continue;
      }
      const referencedIds = [
        sync.addActionId,
        sync.removeActionId,
        sync.actionId,
        sync.verifyActionId,
        ...sync.consumeActionIds
      ].filter(Boolean);
      for (const actionId of referencedIds) {
        assert.ok(actionIds.has(actionId), `${moduleId}.${fieldKey} references missing action ${actionId}`);
      }
    }
  }

  assert.match(
    moduleToml("sevendaystodie"),
    /command_template\s*=\s*"ban add \{\{target\}\} 10 years LanGame"/,
    "7DTD live bans must use the verified long-duration command instead of the immediately-expiring zero duration"
  );
});
