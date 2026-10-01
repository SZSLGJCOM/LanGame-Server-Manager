const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadApi(invoke) {
  const modules = new Map();
  function load(name) {
    if (modules.has(name)) return modules.get(name);
    const filename = path.join(__dirname, "../src", `${name}.ts`);
    const source = fs.readFileSync(filename, "utf8").replaceAll("import.meta.env.DEV", "false");
    const exports = {};
    modules.set(name, exports);
    vm.runInNewContext(transpileTypeScript(source, filename), {
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

// The desktop command uses Tauri's default camelCase argument names. Model
// only that native boundary; execute the real API and transport above it.
function nativeLogCommand(calls) {
  return async (command, args) => {
    assert.equal(command, "read_instance_log_document_from_storage");
    if (typeof args.instanceId !== "string") {
      throw new Error(`command ${command} missing required key instanceId`);
    }
    const request = { instanceId: args.instanceId, maxLines: args.maxLines, runId: args.runId };
    calls.push(request);
    return { source_path: `fixture/run-${request.runId ?? "latest"}.log`, lines: ["server output"] };
  };
}

test("desktop log reads deliver the selected instance and full tail limit", async () => {
  const calls = [];
  const api = loadApi(nativeLogCommand(calls));
  const snapshot = await api.readInstanceLogDocument("instance-a", 400);
  assert.deepEqual(calls, [{ instanceId: "instance-a", maxLines: 400, runId: undefined }]);
  assert.equal(snapshot.source_path, "fixture/run-latest.log");
});

test("shard log reads preserve run selection and the default line limit", async () => {
  const calls = [];
  const api = loadApi(nativeLogCommand(calls));
  const snapshot = await api.readInstanceLogDocument("instance-b", undefined, 42);
  assert.deepEqual(calls, [{ instanceId: "instance-b", maxLines: 200, runId: 42 }]);
  assert.equal(snapshot.source_path, "fixture/run-42.log");
});

test("native log failures remain observable to the panel reader", async () => {
  for (const failure of ["Storage is unavailable", new Error("Storage is unavailable")]) {
    const api = loadApi(async () => { throw failure; });
    await assert.rejects(api.readInstanceLogDocument("instance-a"), error => Object.is(error, failure));
  }
});

test("ARK source selection preserves the map run and source across the native boundary", async () => {
  const calls = [];
  const api = loadApi(async (command, args) => { calls.push({ command, ...args }); return { lines: [] }; });
  await api.readInstanceLogDocument("ark", 400, 23, "game");
  await api.readInstanceLogDocument("ark", 400, 23, "console");
  assert.deepEqual(calls.map(({ instanceId, runId, source, maxLines }) => ({ instanceId, runId, source, maxLines })), [
    { instanceId: "ark", runId: 23, source: "game", maxLines: 400 },
    { instanceId: "ark", runId: 23, source: "console", maxLines: 400 }
  ]);
});
