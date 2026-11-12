const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const filename = path.join(__dirname, "../src/views/servers/instance-removal-presentation.ts");
const loaded = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module: loaded, exports: loaded.exports });
const { formatInstanceRemovalPlan } = loaded.exports;
const catalogFilename = path.join(__dirname, "../src/i18n-messages-en-extra.ts");
const catalog = { exports: {} };
vm.runInNewContext(transpileTypeScript(fs.readFileSync(catalogFilename, "utf8"), catalogFilename),
  { module: catalog, exports: catalog.exports });
const t = (key) => catalog.exports.EN_US_EXTRA_MESSAGES[key];

test("removal keeps consequences and retention clear without listing every path", () => {
  const text = formatInstanceRemovalPlan({ data_path: "D:/instances/one", program_path: "D:/games/example",
    remove_program: false, preserved_program_path: "D:/games/example", owned_data_paths: ["D:/instances/one"],
    preserved_external_saves_path: "D:/external/saves" }, t);
  assert.match(text, /saves and backups will also be deleted/);
  assert.match(text, /cannot be undone/);
  assert.match(text, /last instance also removes unused programs/);
  assert.match(text, /base library is kept/);
  assert.match(text, /External saves are kept/);
  assert.doesNotMatch(text, /D:\//);
  assert.ok(text.length < 200, "The card warning must stay concise");
});

test("exclusive program cleanup stays explicit without inventing external saves", () => {
  const text = formatInstanceRemovalPlan({ data_path: "D:/instances/two", program_path: "D:/programs/two",
    remove_program: true, preserved_program_path: null, owned_data_paths: [], preserved_external_saves_path: null }, t);
  assert.match(text, /exclusive program is also deleted/);
  assert.doesNotMatch(text, /External saves|D:\//);
});
