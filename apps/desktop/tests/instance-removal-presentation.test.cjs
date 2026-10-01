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
const t = (key, values) => `${key}${values ? `\n${values.paths ?? values.path}` : ""}`;

test("removal identifies a related library without promising that unused extra programs remain", () => {
  const text = formatInstanceRemovalPlan({ data_path: "D:/instances/one", program_path: "D:/games/example",
    remove_program: false, preserved_program_path: "D:/games/example", owned_data_paths: ["D:/instances/one"],
    preserved_external_saves_path: "D:/external/saves" }, t);
  const [removed, preserved] = text.split("servers.removal.preserved");
  assert.match(removed, /D:\/instances\/one/);
  assert.doesNotMatch(removed, /D:\/games\/example/);
  assert.match(preserved, /D:\/games\/example/);
  assert.match(preserved, /D:\/external\/saves/);
  assert.match(text, /servers\.removal\.libraryProgram\nD:\/games\/example/);
  assert.match(text, /servers\.removal\.cleanupHint/);
  assert.equal((text.match(/D:\/instances\/one/g) || []).length, 1);
});

test("exclusive installation cleanup names the additional program path", () => {
  const text = formatInstanceRemovalPlan({ data_path: "D:/instances/two", program_path: "D:/programs/two",
    remove_program: true, preserved_program_path: null, owned_data_paths: [], preserved_external_saves_path: null }, t);
  assert.match(text, /D:\/programs\/two/);
  assert.doesNotMatch(text, /servers.removal.preserved/);
});
