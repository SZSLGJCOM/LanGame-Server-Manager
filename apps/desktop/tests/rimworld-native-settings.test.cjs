const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const root = path.resolve(__dirname, "../../..");
const schema = JSON.parse(fs.readFileSync(path.join(root, "modules/rimworld/schema.json"), "utf8"));
const inventory = JSON.parse(fs.readFileSync(path.join(root, "modules/rimworld/native-settings-26.8.31.1.json"), "utf8"));

test("all 103 official shared settings are optional typed overrides with explicit native paths", () => {
  let count = 0;
  function check(fields, file, prefix = []) {
    for (const [name, field] of Object.entries(fields)) {
      if (field.fields) { check(field.fields, file, [...prefix, name]); continue; }
      count += 1;
      const property = schema.properties[field.schema_key];
      assert.ok(property, `${file}:${name}`);
      assert.equal(property.type, field.type.startsWith("float") ? "number" : field.type);
      assert.equal(Object.hasOwn(property, "default"), false, `${field.schema_key} must preserve existing native choices`);
      assert.equal(property["x-lsgm-source-key"], `${file}#${[...prefix, name].join(".")}`);
      assert.ok(["room", "world"].includes(property["x-lsgm-section"]));
      assert.equal(property["x-lsgm-player-access-kind"], undefined);
    }
  }
  for (const record of inventory.records.filter((record) => record.classification === "configuration")) check(record.fields, record.path);
  assert.equal(count, 103);
  assert.ok(inventory.records.some((record) => record.path === "Assets/WorldValuesFile.json" && record.classification === "world_state"));
});

test("password is a room setting with ASCII validation and no implicit clearing default", () => {
  const property = schema.properties.server_password;
  assert.equal(property["x-lsgm-section"], "room");
  assert.equal(property.format, "password");
  assert.equal(Object.hasOwn(property, "default"), false);
  const accepted = new RegExp(property.pattern);
  assert.ok(accepted.test("Join-2026!"));
  for (const value of ["汉字", "spaces here", "newline\n"]) assert.equal(accepted.test(value), false);
  assert.equal(schema.properties.sync_local_save["x-lsgm-source-key"], "UseClientSave");
});
