const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function readSchema(moduleId) {
  return JSON.parse(read(path.join("modules", moduleId, "schema.json")));
}

function fixtureFiles(moduleId) {
  const directory = path.join(repoRoot, "modules", moduleId, "config-fixtures");
  return fs.existsSync(directory)
    ? fs.readdirSync(directory).filter((entry) => entry.endsWith(".json"))
    : [];
}

function iniKeys(contents) {
  let section = "";
  const keys = [];
  for (const rawLine of contents.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith(";") || line.startsWith("#")) continue;
    if (line.startsWith("[") && line.endsWith("]")) {
      section = line.slice(1, -1).trim();
      continue;
    }
    const separator = line.indexOf("=");
    if (separator < 0) continue;
    const key = line.slice(0, separator).trim();
    keys.push(section ? `${section}.${key}` : key);
  }
  return keys.sort();
}

function propertyKeys(contents) {
  return contents
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#") && !line.startsWith("//"))
    .map((line) => line.split(/\s+/, 1)[0])
    .sort();
}

test("Return to Moria reference fixture retains configuration facts without upstream prose", () => {
  const defaults = read("modules/returntomoria/reference-configs/server-defaults-build-21872765.ini");
  const lines = defaults.split(/\r?\n/).filter((line) => line.trim());
  assert.ok(
    lines.every((line) => /^\[[A-Za-z][A-Za-z0-9.]*\]$|^[A-Za-z][A-Za-z0-9.]*=.*$/.test(line)),
    "public reference fixtures contain only section names and key/value facts"
  );
  assert.equal(lines.filter((line) => line.startsWith("[")).length, 6);
  assert.equal(iniKeys(defaults).length, 25);

  const ledger = read("modules/returntomoria/config-sources.toml");
  const source = ledger.split("[[sources]]").find((entry) =>
    entry.includes('id = "first_launch_defaults_21872765"')
  );
  assert.ok(source, "normalized defaults must retain their provenance record");
  const digest = source.match(/^sha256 = "([a-f0-9]{64})"$/m)?.[1];
  assert.ok(digest, "the provenance record must identify the distributed fixture bytes");
  assert.equal(createHash("sha256").update(defaults).digest("hex"), digest);
});

test("Return to Moria matches normalized first-launch defaults and official operator surfaces", () => {
  const schema = readSchema("returntomoria");
  assert.equal(Object.keys(schema.properties).length, 26);

  const defaults = read("modules/returntomoria/reference-configs/server-defaults-build-21872765.ini");
  const rendered = read("modules/returntomoria/templates/MoriaServerConfig.ini.hbs");
  assert.deepEqual(iniKeys(rendered), iniKeys(defaults));
  for (const nativeKey of [
    "OptionalPassword",
    "Name",
    "OptionalWorldFilename",
    "Type",
    "Seed",
    "Difficulty.Preset",
    "Difficulty.Custom.CombatDifficulty",
    "Difficulty.Custom.EnemyAggression",
    "Difficulty.Custom.SurvivalDifficulty",
    "Difficulty.Custom.MiningDrops",
    "Difficulty.Custom.WorldDrops",
    "Difficulty.Custom.HordeFrequency",
    "Difficulty.Custom.SiegeFrequency",
    "Difficulty.Custom.PatrolFrequency",
    "OptionalDLC.Array",
    "UpgradeOptionalDLC.Array",
    "ListenAddress",
    "ListenPort",
    "AdvertiseAddress",
    "AdvertisePort",
    "InitialConnectionRetryTime",
    "AfterDisconnectionRetryTime",
    "Enabled",
    "ServerFPS",
    "LoadedAreaLimit"
  ]) {
    assert.match(defaults, new RegExp(`^${nativeKey.replaceAll(".", "\\.")}=`, "m"));
    assert.match(rendered, new RegExp(`^${nativeKey.replaceAll(".", "\\.")}=`, "m"));
  }
  assert.match(read("modules/returntomoria/templates/MoriaServerRules.txt.hbs"), /rules_message/);
  assert.match(
    read("modules/returntomoria/templates/MoriaServerPermissions.txt.hbs"),
    /permissions_lines/
  );
  assert.match(
    read("crates/app-storage/src/templates_materialize.rs"),
    /materialize_returntomoria_support_files[\s\S]*merge_rendered_ini_file[\s\S]*&\["Server"\]/
  );
});

test("The Forest exact build uses the consumed native key names and defaults", () => {
  const schema = readSchema("theforest");
  const rendered = read("modules/theforest/templates/server.cfg.hbs");
  const upstream = read("modules/theforest/reference-configs/upstream-server.cfg");

  assert.equal(schema.properties.max_players.default, 4);
  assert.ok(schema.properties.max_players.maximum >= 8);
  assert.match(rendered, /^allowCheats /m);
  assert.doesNotMatch(rendered, /^allowCheat /m);
  assert.match(upstream, /^serverPlayers 4$/m);
  assert.match(upstream, /^allowCheats off$/m);
  assert.equal(Object.hasOwn(schema.properties, "server_contact"), false);
  assert.deepEqual(
    propertyKeys(rendered),
    propertyKeys(upstream).filter((key) => key !== "serverContact")
  );
  assert.deepEqual(
    Object.values(schema.properties)
      .filter((property) => property["x-lsgm-source"] === "server_cfg")
      .map((property) => property["x-lsgm-source-key"])
      .sort(),
    propertyKeys(rendered).filter(
      (key) => !["serverIP", "serverSteamPort", "serverGamePort", "serverQueryPort", "saveFolderPath"].includes(key)
    )
  );
});

test("all native-output cohort modules declare versioned acceptance fixtures", () => {
  for (const moduleId of [
    "barotrauma",
    "returntomoria",
    "rust",
    "sevendaystodie",
    "squad",
    "theforest",
    "valheim",
  ]) {
    const files = fixtureFiles(moduleId);
    assert.ok(files.length > 0, `${moduleId} has no native-output fixture`);
    for (const fileName of files) {
      assert.match(fileName, /^\d{4}-\d{2}-\d{2}-.+\.json$/);
      const fixture = JSON.parse(
        read(path.join("modules", moduleId, "config-fixtures", fileName))
      );
      assert.equal(fixture.module_id, moduleId);
      assert.ok(fixture.expected.files.length > 0);
      assert.ok(
        fixture.expected.launch.executable_suffix || fixture.expected.launch.arguments.length > 0
      );
    }
  }
});
