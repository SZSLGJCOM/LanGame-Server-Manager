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
      if (specifier === "../../../domain/live-player-state") return load("domain/live-player-state.ts");
      if (specifier === "./player-access-selected-target") return load("views/servers/player-center/player-access-selected-target.ts");
      if (specifier === "./player-access-roster-model") return { isRosterRecord: (value) => value !== null && typeof value === "object" && !Array.isArray(value) };
      return require(specifier);
    }
  }, { filename });
  return module.exports;
}
const domain = load("domain/player-access.ts");
const selected = load("views/servers/player-center/player-access-selected-target.ts");
const selection = load("views/servers/player-center/player-center-selection.ts");
const property = {
  type: "string", format: "textarea", default: "",
  "x-lsgm-player-access-kind": "admin", "x-lsgm-player-access-codec": "humanitz_net_id",
  "x-lsgm-player-access-sync": { mode: "restart" }
};
const epic = "0123456789abcdef0123456789ABCDEF";
const product = "FEDCBA9876543210fedcba9876543210";
const full = `${epic}|${product}`;
const productOnly = `|${product}`;
const oldSteam = "76561198000000001";
const plain = (value) => JSON.parse(JSON.stringify(value));

test("HumanitZ NetID preserves both native parts, the separator and case", () => {
  assert.equal(domain.parsePlayerAccessCodec(property), "humanitz_net_id");
  for (const value of [full, productOnly]) {
    assert.deepEqual(plain(domain.normalizePlayerAccessMutationEntry("humanitz_net_id", value, property, "add")), {
      identity: value, stored: value
    });
    assert.equal(domain.canonicalPlayerAccessAdditionIdentity("humanitz_net_id", value, property), value);
  }
  assert.notEqual(domain.canonicalPlayerAccessIdentity("humanitz_net_id", full, property),
    domain.canonicalPlayerAccessIdentity("humanitz_net_id", full.toLowerCase(), property));
});

test("new NetID entries reject Steam IDs, partial IDs and unsafe or malformed composites", () => {
  for (const value of [oldSteam, epic, product, `${epic}|`, "|", `${epic}||${product}`,
    `x|${product}`, `|${product}x`, `|${"g".repeat(32)}`, `${full};kick all`,
    `${full}\n`, `${full}\t`, `{{${full}}}`, 42, null]) {
    assert.equal(domain.normalizePlayerAccessMutationEntry("humanitz_net_id", value, property, "add"), null, String(value));
    assert.equal(domain.canonicalPlayerAccessAdditionIdentity("humanitz_net_id", value, property), null, String(value));
  }
});

test("stored Steam IDs remain visible and removable without becoming new EOS identities", () => {
  assert.equal(domain.canonicalPlayerAccessIdentity("humanitz_net_id", oldSteam, property), oldSteam);
  const original = `${oldSteam}\n${full}`;
  assert.deepEqual(Array.from(domain.decodePlayerAccessEntries("admins", property, original), (entry) => entry.rawValue), [oldSteam, full]);
  assert.equal(domain.applyPlayerAccessMutation(property, original, "add", productOnly).value,
    `${original}\n${productOnly}`);
  assert.equal(domain.applyPlayerAccessMutation(property, original, "remove", oldSteam).value, full);
  assert.throws(() => domain.applyPlayerAccessMutation(property, original, "add", oldSteam), /codec/i);
});

test("a mixed invalid stored NetID roster is rejected without filtering old lines", () => {
  for (const value of ["not-a-net-id", product, `${full},${productOnly}`, `${epic}|broken`]) {
    for (const operation of ["add", "remove"]) {
      assert.throws(() => domain.applyPlayerAccessMutation(property, `${oldSteam}\n${value}`, operation, full), /stored.*codec/i);
    }
  }
});

test("live player names, Steam IDs and EOS PUIDs cannot be substituted for a native composite NetID", () => {
  const field = { key: "admin_steam_ids", property };
  for (const identifiers of [[], [{ kind: "player_name", value: full, stable: false }],
    [{ kind: "steam_id", value: oldSteam, stable: true }], [{ kind: "eos_id", value: product, stable: true }],
    [{ kind: "eos_id", value: full, stable: true }]]) {
    assert.equal(selected.resolveSelectedRosterTarget({ identifiers }, field), null);
  }
  const source = { field, entry: { key: full, rawValue: full, label: full } };
  assert.equal(selection.rosterEntryPlayer(source), null);
  assert.equal(selection.resolveRosterActionTarget(null, source, { key: "reserved_player_steam_ids", property }).rawValue, full);
  assert.equal(selection.resolveRosterActionTarget(null,
    { ...source, entry: { key: oldSteam, rawValue: oldSteam, label: oldSteam } },
    { key: "reserved_player_steam_ids", property }), null);
});
