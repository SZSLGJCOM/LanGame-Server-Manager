const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadApi(invoke) {
  function load(name) {
    const filename = path.join(__dirname, "../src", `${name}.ts`);
    const exports = {};
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename, { development: false }), {
      exports,
      require(id) {
        if (id === "@tauri-apps/api/core") return { isTauri: () => true, invoke };
        if (id === "./desktop-exit-lifecycle") return require("./helpers/desktop-exit-fixture.cjs")
          .loadDesktopExitModule({ isTauri: () => true, invoke });
        if (["./api-transport", "./storage-management-requests", "./i18n-config"].includes(id)) return load(id.slice(2));
        if (id === "./locale-preference") return { readPreferredLocale: () => "en-US" };
        throw new Error(`Unexpected dependency: ${id}`);
      }
    }, { filename });
    return exports;
  }
  return load("api");
}

test("creation delegates clean program preparation to the backend without a source override", async () => {
  const requests = [];
  const provisioning = { summary: { id: "created" } };
  const api = loadApi(async (command, args) => {
    requests.push(JSON.parse(JSON.stringify({ command, args })));
    return provisioning;
  });
  assert.equal(await api.createInstance({ name: "Default", module_id: "minecraft" }), provisioning);
  await api.createInstance({ name: "Independent", module_id: "minecraft", program_mode: "independent" });
  assert.deepEqual(requests, [
    { command: "create_instance_record", args: { input: { name: "Default", module_id: "minecraft" } } },
    { command: "create_instance_record", args: { input: { name: "Independent", module_id: "minecraft" }, programMode: "independent" } }
  ]);
});

test("program preparation failures retain their native cause", async () => {
  const failure = new Error("No verified local program is available; install or verify the library program.");
  const api = loadApi(async () => { throw failure; });
  await assert.rejects(api.createInstance({ name: "Missing", module_id: "minecraft" }), (error) => error === failure);
});
