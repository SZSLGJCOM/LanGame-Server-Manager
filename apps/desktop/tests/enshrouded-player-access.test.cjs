const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const sourceRoot = path.resolve(__dirname, "../src");
function load(relative) {
  const filename = path.join(sourceRoot, relative);
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    module, exports: module.exports, URL,
    require(specifier) {
      if (specifier === "../../../domain/player-access") return load("domain/player-access.ts");
      return require(specifier);
    }
  }, { filename });
  return module.exports;
}
const domain = load("domain/player-access.ts");
const selection = load("views/servers/player-center/player-access-selected-target.ts");
const property = {
  type: "string", format: "textarea", default: "",
  "x-lsgm-player-access-kind": "block", "x-lsgm-player-access-codec": "uint64",
  "x-lsgm-player-access-entry-separators": ["newline", "comma"],
  "x-lsgm-player-access-sync": { mode: "restart" }
};
const plain = (value) => JSON.parse(JSON.stringify(value));
const maximum = "18446744073709551615";

test("unsigned account hashes keep every decimal digit and the original stored spelling", () => {
  assert.equal(domain.parsePlayerAccessCodec(property), "uint64");
  for (const value of ["0", "1", "00042", "0000000000000000000000001", "76561198000000001", "9007199254740993", maximum]) {
    assert.equal(domain.canonicalPlayerAccessIdentity("uint64", value, property), value);
    assert.deepEqual(plain(domain.normalizePlayerAccessMutationEntry("uint64", value, property, "add")), {
      identity: value, stored: value
    });
  }
});

test("unsigned account hashes reject malformed and out-of-range input without numeric coercion", () => {
  for (const value of ["", " ", "-1", "+1", "1.5", "1e3", "0x10", "１８", "1_000",
    "18446744073709551616", "999999999999999999999999", "1,2", "1\n2", "1 2", "1\u200b", 42, null]) {
    assert.equal(domain.canonicalPlayerAccessIdentity("uint64", value, property), null, String(value));
    assert.equal(domain.normalizePlayerAccessMutationEntry("uint64", value, property, "add"), null, String(value));
  }
});

test("comma and line separated stored hashes survive additions and removals exactly", () => {
  const original = `76561198000000001, ${maximum}\n00042\n0`;
  const decoded = domain.decodePlayerAccessEntries("banned_player_ids", property, original);
  assert.deepEqual(Array.from(decoded, (entry) => entry.rawValue), ["76561198000000001", maximum, "00042", "0"]);
  const added = domain.applyPlayerAccessMutation(property, original, "add", "9007199254740993");
  assert.equal(added.value, `76561198000000001\n${maximum}\n00042\n0\n9007199254740993`);
  const removed = domain.applyPlayerAccessMutation(property, added.value, "remove", maximum);
  assert.equal(removed.value, "76561198000000001\n00042\n0\n9007199254740993");
  assert.equal(domain.applyPlayerAccessMutation(property, "", "add", "0").value, "0");
  assert.equal(domain.applyPlayerAccessMutation(property, "0", "remove", "0").value, "");
  assert.deepEqual(plain(domain.applyPlayerAccessMutation(property, original, "add", maximum)), {
    changed: true, value: `76561198000000001\n${maximum}\n00042\n0`
  });
  assert.equal(domain.applyPlayerAccessMutation(property, `00042,00042,42`, "add", "0").value, "00042\n42\n0");
  const commented = `# native hashes\n// existing record\n${maximum},\n`;
  assert.equal(domain.applyPlayerAccessMutation(property, commented, "add", "0").value, `${maximum}\n0`);
});

test("Enshrouded exposes native account hashes through its persistent player roster", () => {
  const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules/enshrouded/schema.json"), "utf8"));
  const declared = schema.properties.banned_player_ids;
  for (const key of ["type", "format", "x-lsgm-player-access-kind", "x-lsgm-player-access-codec",
    "x-lsgm-player-access-entry-separators", "x-lsgm-player-access-sync"]) {
    assert.deepEqual(declared[key], property[key]);
  }
  assert.match(declared.title, /Native Account Hashes/);
  assert.equal(declared["x-lsgm-source-key"], "bans");
});

test("configuration validation leaves account hashes to the player roster and still checks custom roles", () => {
  const definition = load("views/settings/modules/enshrouded.ts").enshroudedSettingsDefinition;
  const validate = (key, value) => definition.getFieldValidationMessage({
    field: { key }, value, t: (_key, _parameters, fallback) => fallback
  });
  for (const value of ["0", "9007199254740993", maximum]) {
    assert.equal(validate("banned_player_ids", value), undefined);
  }
  assert.match(validate("custom_user_groups_json", "{"), /JSON object/);
  assert.equal(validate("custom_user_groups_json", '[{"name":"Helper"}]'), undefined);
});

test("a mixed invalid stored roster cannot be silently filtered by a valid mutation", () => {
  for (const invalid of ["invalid", "18446744073709551616", "-1", "1e3"]) {
    const stored = `76561198000000001,${invalid}\n${maximum}`;
    for (const operation of ["add", "remove"]) {
      assert.throws(() => domain.applyPlayerAccessMutation(property, stored, operation, maximum), /stored.*codec/i);
    }
  }
});

test("native account hashes cannot be inferred from a selected Steam identity", () => {
  const player = { identifiers: [{ kind: "steam_id", value: "76561198000000001", stable: true }] };
  const field = { key: "banned_player_ids", property };
  assert.equal(selection.resolveSelectedRosterTarget(player, field), null);
  assert.equal(selection.resolveSelectedRosterIdentity(player, field), null);
});
