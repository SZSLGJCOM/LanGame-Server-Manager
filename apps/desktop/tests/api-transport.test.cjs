const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadDesktopExitModule } = require("./helpers/desktop-exit-fixture.cjs");

function fixture({ development = false, native = false, hostname = "127.0.0.1", protocol = "http:",
  nativeInvoke, token = null, fragment = "", response = { ok: false, status: 401, payload: { ok: false, error: "authentication required" } } } = {}) {
  const calls = [];
  const storage = new Map(token ? [["langameLanToken", token]] : []);
  const location = { hostname, protocol, hash: fragment, pathname: "/", search: "" };
  const window = {
    location,
    sessionStorage: { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) },
    history: { replaceState: (_state, _unused, url) => { calls.push({ kind: "history", url }); location.hash = ""; } }
  };
  const filename = path.join(__dirname, "../src/api-transport.ts");
  const source = fs.readFileSync(filename, "utf8").replaceAll("import.meta.env.DEV", String(development));
  const exports = {};
  const core = { isTauri: () => native, invoke: async (command) => {
    calls.push({ kind: "native", command }); return nativeInvoke ? nativeInvoke(command) : "native";
  } };
  const exit = loadDesktopExitModule(core);
  vm.runInNewContext(transpileTypeScript(source, filename), {
    exports, window, URLSearchParams,
    fetch: async (url, options) => {
      calls.push({ kind: "fetch", url, options });
      if (response instanceof Error) throw response;
      return { ok: response.ok, status: response.status, json: async () => response.payload };
    },
    require(id) {
      if (id === "@tauri-apps/api/core") return core;
      if (id === "./desktop-exit-lifecycle") return exit;
      if (id === "./api-mock") return { invokeMock: async () => { calls.push({ kind: "mock" }); return "preview"; } };
      throw new Error(`Unexpected dependency: ${id}`);
    }
  }, { filename });
  return { api: exports, calls, storage, location };
}

for (const hostname of ["localhost", "127.0.0.1", "::1", "[::1]", "server.example"]) {
  test(`production ${hostname} reports authentication failure without substituting preview servers`, async () => {
    const { api, calls } = fixture({ hostname });
    await assert.rejects(api.invokeOrMock("bootstrap"), /authentication required/);
    assert.deepEqual(calls.map((call) => call.kind), ["fetch"]);
  });
}

test("native host takes precedence over browser credentials and development mode", async () => {
  const { api, calls } = fixture({ native: true, development: true, token: "synthetic-test-only" });
  assert.equal(await api.invokeOrMock("bootstrap"), "native");
  assert.deepEqual(calls.map((call) => call.kind), ["native"]);
});

test("native final-exit rejection latches the UI and prevents further runtime requests without hiding its error", async () => {
  const original = "runtime overview reconciliation cannot begin while application shutdown is in progress";
  const { api, calls } = fixture({ native: true, nativeInvoke: async (command) => {
    if (command === "app_exit_status") return { requested: true };
    throw original;
  } });
  await assert.rejects(api.invokeOrMock("read_instance_runtime"), (error) => error === original);
  await assert.rejects(api.invokeOrMock("bootstrap"), /Application final exit is in progress/);
  assert.deepEqual(calls.map((call) => call.command), ["read_instance_runtime", "app_exit_status"]);
});

test("recoverable runtime shutdown keeps the original error and permits a later read", async () => {
  let rejected = false;
  const original = "instance detail reconciliation cannot begin while application shutdown is in progress";
  const { api } = fixture({ native: true, nativeInvoke: async (command) => {
    if (command === "app_exit_status") return { requested: false };
    if (!rejected) { rejected = true; throw original; }
    return "recovered";
  } });
  await assert.rejects(api.invokeOrMock("read_instance_details"), (error) => error === original);
  assert.equal(await api.invokeOrMock("read_instance_details"), "recovered");
});

test("only development loopback preview uses synthetic data", async () => {
  const { api, calls } = fixture({ development: true, hostname: "[::1]" });
  assert.equal(await api.invokeOrMock("bootstrap"), "preview");
  assert.deepEqual(calls.map((call) => call.kind), ["mock"]);
  const remote = fixture({ development: true, hostname: "server.example" });
  await assert.rejects(remote.api.invokeOrMock("bootstrap"), /authentication required/);
  assert.deepEqual(remote.calls.map((call) => call.kind), ["fetch"]);
});

test("fragment token is cleared and sent only to the management header", async () => {
  const { api, calls, location, storage } = fixture({ development: true, fragment: "#langameToken=synthetic-test-only",
    response: { ok: true, status: 200, payload: { ok: true, value: "real result" } } });
  assert.equal(await api.invokeOrMock("read_instance_details_from_storage", { instanceId: "fixture" }), "real result");
  assert.equal(location.hash, "");
  assert.equal(storage.get("langameLanToken"), "synthetic-test-only");
  const request = calls.find((call) => call.kind === "fetch");
  assert.equal(request.url, "/__langame/api");
  assert.equal(request.options.headers["X-LanGame-Token"], "synthetic-test-only");
  assert.equal(request.options.redirect, "error", "a redirect cannot forward the management header to another origin");
  assert.equal(request.options.credentials, "omit");
  assert.equal(request.options.cache, "no-store");
  assert.ok(!request.options.body.includes("synthetic-test-only"));
});

test("network failures and malformed responses never report success or retry a mutation", async () => {
  for (const response of [new Error("network unavailable"),
    { ok: true, status: 200, payload: null },
    { ok: true, status: 200, payload: { ok: "true", value: "wrong shape" } },
    { ok: true, status: 200, payload: { ok: true } }]) {
    const { api, calls } = fixture({ response });
    await assert.rejects(api.invokeOrMock("start_instance_process", { instanceId: "fixture" }));
    assert.deepEqual(calls.map((call) => call.kind), ["fetch"]);
  }
});

test("a production page without a supported host cannot fall back to preview", async () => {
  const { api, calls } = fixture({ protocol: "file:" });
  await assert.rejects(api.invokeOrMock("bootstrap"), /requires its desktop host/);
  assert.equal(calls.length, 0);
});
