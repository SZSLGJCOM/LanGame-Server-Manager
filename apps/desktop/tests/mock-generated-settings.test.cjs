const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const modulesRoot = path.resolve(__dirname, "../../../modules");
const schemas = Object.fromEntries(fs.readdirSync(modulesRoot)
  .filter((id) => fs.existsSync(path.join(modulesRoot, id, "schema.json")))
  .map((id) => [id, JSON.parse(fs.readFileSync(path.join(modulesRoot, id, "schema.json"), "utf8"))]));
const filename = path.resolve(__dirname, "../src/api-mock/module-settings.ts");
const loaded = new Module(filename, module);
loaded.filename = filename;
loaded.require = (id) => {
  assert.equal(id, "./module-assets");
  return { mockModuleSchemasById: schemas };
};
loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);

test("browser instance creation honors every native generated-secret setting", () => {
  const seen = new Set();
  for (const [id, schema] of Object.entries(schemas)) {
    const fields = Object.entries(schema.properties ?? {}).filter(([, property]) => property["x-lsgm-default-source"] === "generated_secret");
    for (const instanceId of ["instance-one", "instance-two"]) {
      const settings = loaded.exports.buildMockSettingsForModule(id, "Local co-op", instanceId);
      for (const [key, property] of fields) {
        const value = settings[key];
        const expectedLength = Math.min(48, property["x-lsgm-generated-secret-length"] ?? 48, property.maxLength ?? 48);
        assert.match(value, /^[a-f0-9]+$/, `${id}:${key} must initialize an argument-safe secret`);
        assert.equal(value.length, expectedLength, `${id}:${key} must honor its schema length limit`);
        assert.equal(seen.has(value), false, "instances and fields must not share generated credentials");
        seen.add(value);
      }
    }
  }
});

test("browser generated-secret length changes defaults without imposing an input limit", () => {
  const cases = [
    { property: { "x-lsgm-generated-secret-length": 28 }, expected: 28 },
    { property: { "x-lsgm-generated-secret-length": 28, maxLength: 20 }, expected: 20 },
    { property: { "x-lsgm-generated-secret-length": 28, maxLength: 32 }, expected: 28 },
    { property: { maxLength: 28 }, expected: 28 },
    { property: {}, expected: 48 },
    ...[0, -1, 1.5, "28", Number.MAX_SAFE_INTEGER + 1].map((value) => ({
      property: { "x-lsgm-generated-secret-length": value }, expected: 48
    }))
  ];
  schemas.password_limit_fixture = {
    properties: Object.fromEntries(cases.map(({ property }, index) => [
      `password_${index}`, { type: "string", "x-lsgm-default-source": "generated_secret", ...property }
    ]))
  };
  try {
    const settings = loaded.exports.buildMockSettingsForModule("password_limit_fixture", "Local co-op", "bounded-secret");
    for (const [index, { expected }] of cases.entries()) {
      assert.equal(settings[`password_${index}`].length, expected, `length fixture ${index}`);
      assert.match(settings[`password_${index}`], /^[a-f0-9]+$/);
    }
  } finally {
    delete schemas.password_limit_fixture;
  }
});
