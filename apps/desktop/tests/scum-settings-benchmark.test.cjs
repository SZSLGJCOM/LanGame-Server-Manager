const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "..", "..", "..");
const moduleRoot = path.join(root, "modules", "scum");
const inventoryRoot = path.join(moduleRoot, "server-settings-v7");
const expectedSections = {
  General: 66,
  World: 142,
  Features: 135,
  Respawn: 29,
  Vehicles: 46,
  Damage: 19
};

function readInventory() {
  return Object.keys(expectedSections).flatMap((section) => JSON.parse(fs.readFileSync(
    path.join(inventoryRoot, `${section.toLowerCase()}.json`),
    "utf8"
  )));
}

function digest(values) {
  return crypto.createHash("sha256").update([...values].sort().join("\n")).digest("hex");
}

test("SCUM v7 inventory classifies every baseline and official August ServerSettings key once", () => {
  const inventory = readInventory();
  assert.equal(inventory.length, 437);
  assert.equal(new Set(inventory.map((item) => item.nativeKey)).size, 437);
  assert.equal(new Set(inventory.map((item) => item.key)).size, 437);
  assert.equal(digest(inventory.map((item) => item.nativeKey)), "2a51702b2c516b026e61d013d7d4fa4d4cb6a54a14ad51ba860e501bb3642ba4");

  for (const [section, count] of Object.entries(expectedSections)) {
    assert.equal(inventory.filter((item) => item.section === section).length, count);
  }
  assert.deepEqual(
    inventory.filter((item) => item.presentation === "generated").map((item) => item.nativeKey),
    ["scum.ServerSettingsVersion"]
  );
  assert.equal(inventory.filter((item) => item.presentation === "specialized").length, 436);
});

test("SCUM schema stores native sections and three JSON surfaces as structured settings", () => {
  const schema = JSON.parse(fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8"));
  for (const key of [
    "server_general",
    "server_world",
    "server_features",
    "server_respawn",
    "server_vehicles",
    "server_damage",
    "economy_override",
    "raid_times",
    "notifications"
  ]) {
    assert.ok(schema.properties[key], `${key} must be schema-backed`);
    assert.ok(["object", "array"].includes(schema.properties[key].type));
  }
  assert.equal(schema.properties.extra_launch_args.type, "string");
  assert.equal(schema.properties.admin_steam_ids["x-lsgm-player-access-kind"], "admin");
});

test("SCUM structured editors do not invent native JSON row defaults", () => {
  const rendererSource = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "ScumJsonSettingsRenderer.tsx"
  ), "utf8");

  assert.doesNotMatch(rendererSource, /key\.includes\("price"\) \? "-1" : "default"/u);
  assert.match(rendererSource, /Object\.fromEntries\(TRADEABLE_FIELDS\.map\(\(key\) => \[key, ""\]\)\)/u);
});

test("SCUM Configuration renderer exposes one searchable focus target per native key", () => {
  const moduleSource = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "modules",
    "scum.ts"
  ), "utf8");
  const rendererSource = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "ScumServerSettingsRenderer.tsx"
  ), "utf8");
  const parserSource = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "guided-settings.ts"
  ), "utf8");

  assert.match(moduleSource, /getAdditionalPresentationFields/u);
  assert.match(parserSource, /getAdditionalPresentationFields/u);
  assert.match(rendererSource, /buildConfigurationFieldIds/u);
  assert.match(rendererSource, /data-scum-native-key/u);
  assert.match(rendererSource, /nativeKey/u);
});

test("SCUM specialized editors keep implementation evidence out of the player-facing fields", () => {
  const settingsRoot = path.join(root, "apps", "desktop", "src", "views", "settings");
  const serverRenderer = fs.readFileSync(path.join(settingsRoot, "ScumServerSettingsRenderer.tsx"), "utf8");
  const jsonRenderer = fs.readFileSync(path.join(settingsRoot, "ScumJsonSettingsRenderer.tsx"), "utf8");

  for (const source of [serverRenderer, jsonRenderer]) {
    assert.match(source, /useConfigurationFieldHelp/u);
    assert.doesNotMatch(source, /configuration-field-evidence/u);
  }
  assert.match(serverRenderer, /data-scum-native-key/u);
  assert.match(jsonRenderer, /data-scum-native-key/u);
  assert.doesNotMatch(serverRenderer, />Native key</u);
  assert.doesNotMatch(serverRenderer, />Evidence</u);
  assert.doesNotMatch(serverRenderer, /build 24107648 \/ v7/u);
  assert.doesNotMatch(serverRenderer, /ServerSettings\.ini/u);
  assert.doesNotMatch(serverRenderer, /native keys/u);
  assert.doesNotMatch(serverRenderer, /label or native key/u);
  assert.doesNotMatch(jsonRenderer, /EconomyOverride\.json|RaidTimes\.json|Notifications\.json/u);
  assert.doesNotMatch(jsonRenderer, /Structured JSON|native SCUM JSON contract/u);
});

test("SCUM destructive and internal booleans require stopped-server confirmation", () => {
  const rendererSource = fs.readFileSync(path.join(
    root,
    "apps",
    "desktop",
    "src",
    "views",
    "settings",
    "ScumServerSettingsRenderer.tsx"
  ), "utf8");
  for (const key of ["partial_wipe", "gold_wipe", "full_wipe", "master_server_is_local_test"]) {
    assert.match(rendererSource, new RegExp(`\\b${key}\\b`, "u"));
  }
  assert.match(rendererSource, /serverMustStop/u);
  assert.match(rendererSource, /confirm/u);
  assert.match(
    rendererSource,
    /function confirm\(setting: ScumNativeSetting\) \{\s*if \(serverMustStop \|\| props\.disabled\)/u,
    "confirmation must re-check runtime state to close the start/confirm race"
  );
  assert.match(
    rendererSource,
    /className="ghost-button danger scum-settings-action" disabled=\{props\.disabled \|\| serverMustStop\}/u,
    "confirmation action must disable when the form or server state becomes unsafe"
  );
});

test("SCUM materialization batches managed INI, JSON, and Player Access text", () => {
  const materializer = fs.readFileSync(path.join(
    root,
    "crates",
    "app-storage",
    "src",
    "templates_materialize",
    "scum.rs"
  ), "utf8");
  assert.match(materializer, /merge_rendered_config_files/u);
  assert.match(materializer, /ManagedConfigFile::Ini/u);
  assert.equal((materializer.match(/ManagedConfigFile::JsonObject/gu) ?? []).length, 3);
  assert.match(materializer, /ManagedConfigFile::Text/u);
  assert.match(materializer, /SCUM_ADMIN_USERS_FILE/u);
  assert.doesNotMatch(materializer, /for file_name in \[SCUM_SERVER_SETTINGS_FILE, SCUM_ADMIN_USERS_FILE\]/u);
});
