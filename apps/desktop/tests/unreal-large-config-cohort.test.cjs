const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const moduleIds = ["abioticfactor", "conanexiles", "humanitz", "soulmask"];
const fixtureNames = {
  abioticfactor: "2026-09-08-sandbox_settings.json",
  conanexiles: "2026-07-13-steamcmd_anonymous_validate_443030.json",
  humanitz: "2026-07-13-steamcmd_anonymous_app_2728330.json",
  soulmask: "2026-07-13-launch_args.json"
};

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function readJson(relativePath) {
  return JSON.parse(read(relativePath));
}

function modulePath(moduleId, ...parts) {
  return path.join("modules", moduleId, ...parts);
}

function parseTomlArrayTables(text, tableName) {
  const tables = [];
  const pattern = new RegExp(`\\[\\[${tableName}\\]\\]([\\s\\S]*?)(?=\\n\\[\\[|\\n\\[|$)`, "g");
  for (const match of text.matchAll(pattern)) {
    const table = {};
    for (const line of match[1].split(/\r?\n/)) {
      const item = line.match(/^\s*([A-Za-z0-9_]+)\s*=\s*"([^"]*)"\s*$/);
      if (item) table[item[1]] = item[2];
    }
    tables.push(table);
  }
  return tables;
}

function ledgerTables(moduleId) {
  const ledger = read(modulePath(moduleId, "config-sources.toml"));
  return {
    ledger,
    items: parseTomlArrayTables(ledger, "items"),
    exclusions: parseTomlArrayTables(ledger, "exclusions")
  };
}

function iniKeys(content) {
  let section = "";
  const keys = [];
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim();
    const sectionMatch = line.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      section = sectionMatch[1];
      continue;
    }
    if (!line || /^[;#]/.test(line) || !line.includes("=")) continue;
    const key = line.slice(0, line.indexOf("=")).trim();
    keys.push(section ? `${section}.${key}` : key);
  }
  return keys;
}

function sourceKeys(fixture, root, filePath) {
  const file = fixture.initial.files.find((candidate) => candidate.root === root && candidate.path === filePath);
  assert.ok(file, `missing initial ${root}:${filePath}`);
  assert.equal(typeof file.content, "string", `${filePath} must preserve exact text and ordering`);
  return iniKeys(file.content);
}

test("exact-build acceptance fixtures exist for all Unreal large-config modules", () => {
  for (const moduleId of moduleIds) {
    const fixturePath = modulePath(moduleId, "config-fixtures", fixtureNames[moduleId]);
    assert.ok(fs.existsSync(path.join(repoRoot, fixturePath)), `${moduleId} exact fixture is missing`);
    const fixture = readJson(fixturePath);
    assert.equal(fixture.module_id, moduleId);
    assert.equal(fixture.fixture_version, 1);
    assert.equal(fixture.evidence.verified_at, moduleId === "abioticfactor" ? "2026-09-08" : "2026-07-13");
  }
});

test("Abiotic Factor maps all 59 documented settings with 12 explicit exclusions", () => {
  const schema = readJson(modulePath("abioticfactor", "schema.json"));
  const { ledger, items, exclusions } = ledgerTables("abioticfactor");
  assert.match(ledger, /^status = "exhaustive_verified"/);
  assert.equal(Object.keys(schema.properties).length, 59);
  assert.equal(items.length, 59);
  assert.equal(exclusions.length, 12);
  for (const key of ["-stdout", "-FullStdOutLogOutput"]) {
    assert.ok(exclusions.some((entry) => entry.source === "launch_args" && entry.key === key),
      `${key} is fixed by the managed host, not an editable setting`);
  }
  assert.deepEqual(new Set(items.map((item) => item.schema_key)), new Set(Object.keys(schema.properties)));
  assert.equal(items.filter((item) => item.source === "sandbox_settings").length, 48);
  assert.equal(items.filter((item) => item.source === "admin_ini").length, 1);
  assert.equal(items.filter((item) => item.source === "launch_args").length, 10);
});

test("Conan exact first-run INI keys are individually mapped or explicitly excluded", () => {
  const fixture = readJson(modulePath("conanexiles", "config-fixtures", fixtureNames.conanexiles));
  const { items, exclusions } = ledgerTables("conanexiles");
  const accounted = new Set(
    [...items, ...exclusions]
      .filter((entry) => entry.source === "server_settings")
      .map((entry) => entry.key.toLowerCase())
  );
  const exactKeys = sourceKeys(
    fixture,
    "install",
    "ConanSandbox/Saved/Config/WindowsServer/ServerSettings.ini"
  );
  const missing = exactKeys.filter((key) => !accounted.has(key.toLowerCase()));
  assert.deepEqual(missing, [], `unaccounted Conan exact-build keys: ${missing.join(", ")}`);

  const manifest = read(modulePath("conanexiles", "module.toml"));
  assert.match(manifest, /"-MULTIHOME=\{\{instance\.bind_ip\}\}"/);
  assert.match(manifest, /"-MULTIHOMEHTTP=\{\{instance\.bind_ip\}\}"/);
  assert.doesNotMatch(manifest, /-RconEnabled=/);
  assert.match(
    read(modulePath("conanexiles", "templates", "Game.ini.hbs")),
    /\[RconPlugin\][\s\S]*RconEnabled=\{\{rcon_enabled\}\}/
  );
  assert.ok(
    exclusions.some((entry) => entry.source === "launch_args" && entry.key === "-MULTIHOMEHTTP"),
    "manager-owned MULTIHOMEHTTP needs an exact exclusion"
  );
});

test("HumanitZ native rosters are owned only by Player Access", () => {
  const schema = readJson(modulePath("humanitz", "schema.json"));
  const rosterKeys = [
    "admin_steam_ids",
    "reserved_player_steam_ids",
    "banned_player_steam_ids"
  ];
  for (const key of rosterKeys) {
    const property = schema.properties[key];
    assert.ok(property, `missing ${key}`);
    assert.ok(property["x-lsgm-player-access-kind"], `${key} must be routed to Player Access`);
    assert.equal(property["x-lsgm-player-access-codec"], "humanitz_net_id");
  }
  assert.equal(schema.properties.allowed_player_steam_ids, undefined,
    "native whitelist admission uses the reserved-slot roster, not F_MVPAccess.txt");
  assert.equal(schema.properties.welcome_message["x-lsgm-player-access-kind"], undefined);

  const presentation = read("apps/desktop/src/views/settings/configuration-presentation.ts");
  assert.match(presentation, /readNonEmptyString\(property\["x-lsgm-player-access-kind"\]\)/);
  assert.match(presentation, /owner:\s*"player_access"/);
});

test("Soulmask maps all 276 package keys and uses package-native numeric bounds", () => {
  const schema = readJson(modulePath("soulmask", "schema.json"));
  const { items } = ledgerTables("soulmask");
  const gameplay = Object.entries(schema.properties).filter(([, property]) =>
    property["x-lsgm-source"] === "game_xishu_json"
  );
  assert.equal(gameplay.length, 276);
  assert.equal(items.filter((item) => item.source === "game_xishu_json").length, 276);
  assert.deepEqual(new Set(gameplay.map(([key]) => key)), new Set(
    items.filter((item) => item.source === "game_xishu_json").map((item) => item.schema_key)
  ));
  assert.equal(schema.properties.xishu_zuo_wu_sheng_zhang_ratio.maximum, 100);
  assert.equal(schema.properties.xishu_zhi_zuo_time_ratio.maximum, 100);
  assert.equal(schema.properties.xishu_fu_hua_speed.maximum, 100);
});

test("Unreal large-config source modules stay below the ordinary-file size limit", () => {
  const files = [
    "apps/desktop/src/views/settings/modules/conanexiles.ts",
    "apps/desktop/src/views/settings/modules/conanexiles-support.ts",
    "apps/desktop/src/views/settings/modules/conanexiles-exact-groups.ts",
    "apps/desktop/src/views/settings/modules/soulmask.ts",
    "apps/desktop/src/views/settings/modules/soulmask-groups.ts",
    "modules/soulmask/templates/GameXishu.profile-0.json.hbs",
    "modules/soulmask/templates/GameXishu.profile-1.json.hbs",
    "modules/soulmask/templates/GameXishu.profile-2.json.hbs",
    "crates/app-storage/src/templates_materialize/unreal_large.rs",
    "crates/app-storage/src/templates_materialize/unreal_large_tests.rs",
    "crates/app-storage/src/templates_render_soulmask.rs",
    "crates/app-storage/src/templates_render_soulmask_tests.rs"
  ];
  for (const relativePath of files) {
    const absolutePath = path.join(repoRoot, relativePath);
    assert.ok(fs.existsSync(absolutePath), `${relativePath} is missing`);
    const lines = read(relativePath).split(/\r?\n/).length;
    assert.ok(lines < 500, `${relativePath} has ${lines} lines`);
  }
});
