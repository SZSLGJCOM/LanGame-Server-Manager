const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");

function loadTypeScriptModule(sourcePath, requireDependency = require) {
  const source = fs.readFileSync(sourcePath, "utf8");
  const outputText = transpileTypeScript(source, sourcePath);
  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require: requireDependency,
    URL
  }, { filename: sourcePath });
  return module.exports;
}

function loadMockPlayerAccess() {
  const domain = loadTypeScriptModule(path.join(
    repositoryRoot,
    "apps",
    "desktop",
    "src",
    "domain",
    "player-access.ts"
  ));
  return loadTypeScriptModule(path.join(
    repositoryRoot,
    "apps",
    "desktop",
    "src",
    "api-mock",
    "player-access.ts"
  ), (request) => request === "../domain/player-access" ? domain : require(request));
}

function moduleProperty(moduleId, fieldKey) {
  const schema = JSON.parse(fs.readFileSync(
    path.join(repositoryRoot, "modules", moduleId, "schema.json"),
    "utf8"
  ));
  return schema.properties[fieldKey];
}

function moduleProperties(moduleId) {
  return JSON.parse(fs.readFileSync(
    path.join(repositoryRoot, "modules", moduleId, "schema.json"),
    "utf8"
  )).properties;
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

test("api mock delegates player-access semantics and cannot report fabricated verification", () => {
  const source = fs.readFileSync(path.join(
    repositoryRoot,
    "apps",
    "desktop",
    "src",
    "api-mock.ts"
  ), "utf8");
  assert.match(source, /applyMockPlayerAccessMutation\(property, currentValue/);
  assert.match(source, /expectedValue:\s*input\.expectedValue/);
  assert.doesNotMatch(source, /Mock verification succeeded|verificationStatus:\s*["']verified["']/);
});

test("mock scalar player access uses replace/clear CAS and never fabricates verification", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("unturned", "owner_steam_id");
  const firstOwner = "76561198000000001";
  const secondOwner = "76561198000000002";

  const first = applyMockPlayerAccessMutation(property, "", {
    operation: "add",
    value: firstOwner,
    expectedValue: ""
  }, true);
  assert.deepEqual(plain(first), {
    changed: true,
    liveTarget: firstOwner,
    value: firstOwner,
    liveStatus: "restart_required",
    verificationStatus: "unavailable"
  });

  const direct = applyMockPlayerAccessMutation({
    ...property,
    "x-lsgm-player-access-sync": {
      mode: "direct",
      add_action_id: "add_owner",
      remove_action_id: "remove_owner",
      verify_action_id: "list_owners"
    }
  }, "", {
    operation: "add",
    value: firstOwner,
    expectedValue: ""
  }, true);
  assert.equal(direct.liveStatus, "sent_unverified");
  assert.equal(direct.verificationStatus, "unavailable");

  assert.throws(() => applyMockPlayerAccessMutation(property, firstOwner, {
    operation: "add",
    value: secondOwner,
    expectedValue: ""
  }, true), /changed while this edit was pending/i);

  const replaced = applyMockPlayerAccessMutation(property, firstOwner, {
    operation: "add",
    value: secondOwner,
    expectedValue: firstOwner
  }, true);
  assert.equal(replaced.value, secondOwner);
  assert.equal(replaced.liveTarget, secondOwner);

  assert.throws(() => applyMockPlayerAccessMutation(property, secondOwner, {
    operation: "remove",
    value: firstOwner,
    expectedValue: firstOwner
  }, true), /changed while this edit was pending/i);

  const cleared = applyMockPlayerAccessMutation(property, secondOwner, {
    operation: "remove",
    value: secondOwner,
    expectedValue: secondOwner
  }, true);
  assert.equal(cleared.changed, true);
  assert.equal(cleared.value, "");
  assert.equal(cleared.verificationStatus, "unavailable");
});

test("mock rejects unauthorized fields and values outside the declared codec", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("unturned", "owner_steam_id");

  assert.throws(() => applyMockPlayerAccessMutation({
    ...property,
    "x-lsgm-player-access-codec": "unknown"
  }, "", { operation: "add", value: "76561198000000001" }, false), /not an authorized/i);

  assert.throws(() => applyMockPlayerAccessMutation({
    ...property,
    "x-lsgm-player-access-kind": undefined
  }, "", { operation: "add", value: "76561198000000001" }, false), /not an authorized/i);

  assert.throws(() => applyMockPlayerAccessMutation(
    property,
    "",
    { operation: "add", value: "not-a-steam-id", expectedValue: "" },
    false
  ), /does not match the field codec/i);
});

test("mock object mutation applies defaults and atomically replaces composite identity metadata", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  const steamId = "76561198000000001";

  const added = applyMockPlayerAccessMutation(property, [], {
    operation: "add",
    value: { platform: "Steam", userid: steamId }
  }, false);
  assert.deepEqual(plain(added.value), [{
    platform: "Steam",
    userid: steamId,
    name: "",
    unbandate: "9999-12-31",
    reason: "LanGame"
  }]);
  assert.equal(added.liveStatus, "not_running");
  assert.equal(added.liveTarget, `Steam_${steamId}`);

  const replaced = applyMockPlayerAccessMutation(property, added.value, {
    operation: "add",
    value: {
      platform: "Steam",
      userid: steamId,
      name: " Alice ",
      reason: " Manual review "
    }
  }, true);
  assert.deepEqual(plain(replaced.value), [{
    platform: "Steam",
    userid: steamId,
    name: "Alice",
    unbandate: "9999-12-31",
    reason: "Manual review"
  }]);
  assert.equal(replaced.liveStatus, "restart_required", "Adding to the persistent blacklist must not dispatch its remove-only live command");

  const secondPlatform = applyMockPlayerAccessMutation(property, replaced.value, {
    operation: "add",
    value: { platform: "EOS", userid: steamId }
  }, true);
  assert.equal(secondPlatform.value.length, 2, "platform participates in the composite identity");
  assert.equal(secondPlatform.liveTarget, `EOS_${steamId}`);

  const removed = applyMockPlayerAccessMutation(property, secondPlatform.value, {
    operation: "remove",
    value: { platform: "Steam", userid: steamId }
  }, true);
  assert.deepEqual(plain(removed.value), [{
    platform: "EOS",
    userid: steamId,
    name: "",
    unbandate: "9999-12-31",
    reason: "LanGame"
  }]);
  assert.equal(removed.verificationStatus, "unavailable");
  assert.equal(removed.liveStatus, "sent_unverified");
  assert.equal(removed.liveTarget, `Steam_${steamId}`);
});

test("mock 7DTD removal remains persistent-only while stopped and never supplies a default identity", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  const entry = { platform: "EOS", userid: "0002ab34ef", name: "Alice", unbandate: "2099-12-31", reason: "Existing reason" };
  const result = applyMockPlayerAccessMutation(property, [entry], { operation: "remove", value: entry }, false);
  assert.deepEqual(plain(result.value), []);
  assert.equal(result.liveTarget, "EOS_0002ab34ef");
  assert.equal(result.liveStatus, "not_running");
  assert.equal(result.verificationStatus, "unavailable");
  assert.throws(() => applyMockPlayerAccessMutation(property, [], {
    operation: "add", value: { userid: "76561198000000001" }
  }, true), /valid runtime target|does not match/i);
  for (const value of [{ platform: "Steam", userid: "42" }, { platform: "EOS", userid: "dead" }, { platform: "EOS", userid: "not-a-hex-account" }]) {
    assert.throws(() => applyMockPlayerAccessMutation(property, [], { operation: "add", value }, true), /valid runtime target|does not match/i);
  }
});

test("mock preserves native 7DTD expiry seconds through metadata edits and removal", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("sevendaystodie", "blacklist_entries");
  const nativeEntry = { platform: "Steam", userid: "76561198000000001", name: "Alice",
    unbandate: "2036-02-29 23:59:58", reason: "Native ban" };
  const edited = applyMockPlayerAccessMutation(property, [nativeEntry], {
    operation: "add", value: { ...nativeEntry, reason: "Reviewed ban" }
  }, false);
  assert.equal(edited.value[0].unbandate, nativeEntry.unbandate);
  assert.equal(edited.value[0].reason, "Reviewed ban");
  const removed = applyMockPlayerAccessMutation(property, edited.value, { operation: "remove", value: edited.value[0] }, true);
  assert.deepEqual(plain(removed.value), []);
  assert.equal(removed.liveTarget, `Steam_${nativeEntry.userid}`);
  for (const unbandate of ["2035-02-29 00:00:00", "2036-02-29 24:00:00", "2036-02-29 23:60:00", "2036-02-29 23:59:60"]) {
    assert.throws(() => applyMockPlayerAccessMutation(property, [], {
      operation: "add", value: { ...nativeEntry, unbandate }
    }, false), /does not match the field codec|valid runtime target/i);
  }
});

test("mock rejects declared cross-roster identity conflicts", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const properties = moduleProperties("valheim");
  const settings = {
    admin_list: "",
    banned_list: "",
    permitted_list: "Steam_76561198000000001"
  };

  assert.throws(() => applyMockPlayerAccessMutation(
    properties.banned_list,
    settings.banned_list,
    { operation: "add", value: "steam_76561198000000001" },
    false,
    { fieldKey: "banned_list", properties, settings }
  ), /conflicts with permitted_list/i);

  const removed = applyMockPlayerAccessMutation(
    properties.permitted_list,
    settings.permitted_list,
    { operation: "remove", value: "Steam_76561198000000001" },
    false,
    { fieldKey: "permitted_list", properties, settings }
  );
  assert.equal(removed.value, "");
});


test("HumanitZ retained Steam removal is persistent-only, while complete NetID actions remain direct", () => {
  const { applyMockPlayerAccessMutation } = loadMockPlayerAccess();
  const property = moduleProperty("humanitz", "banned_player_steam_ids");
  const full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
  const old = "76561198000000001";
  const removed = applyMockPlayerAccessMutation(property, `${old}\n${full}`, { operation: "remove", value: old }, true);
  assert.equal(removed.value, full);
  assert.equal(removed.liveStatus, "restart_required");
  assert.equal(removed.verificationStatus, "unavailable");
  assert.equal(applyMockPlayerAccessMutation(property, old, { operation: "add", value: full }, true).liveStatus, "sent_unverified");
  assert.throws(() => applyMockPlayerAccessMutation(property, "", { operation: "add", value: old }, true), /codec/);
});
