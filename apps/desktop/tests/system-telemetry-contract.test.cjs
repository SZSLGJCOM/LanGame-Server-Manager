const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");
const compile = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
require.extensions[".ts"] = compile;
require.extensions[".tsx"] = compile;
const { formatDesktopError } = require("../src/desktop-error-message.ts");
const { translate } = require("../src/i18n.tsx");
const catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const translator = (locale) => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);

function loadApi(value) {
  const filename = path.join(__dirname, "../src/api.ts");
  const calls = [];
  const exports = {};
  const dependencies = {
    "@tauri-apps/api/core": { isTauri: () => true },
    "./locale-preference": {},
    "./storage-management-requests": loadStorageManagementRequests(),
    "./api-transport": { invokeOrMock: async (command, args) => {
      calls.push(JSON.parse(JSON.stringify({ command, args })));
      return value;
    } }
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, Error, require: (name) => { assert.ok(name in dependencies, name); return dependencies[name]; }
  }, { filename });
  return { bootstrapApp: exports.bootstrapApp, calls };
}

function booted(telemetry) {
  return { booted_at_unix_ms: 1, state: { snapshot: {
    cpu_percent: 40, memory_total_bytes: 32 * 1024 ** 3, memory_available_bytes: 20 * 1024 ** 3,
    network_receive_bps: 1024, network_transmit_bps: 2048,
    ...(telemetry === undefined ? {} : { telemetry })
  } } };
}

const cold = { observed_at_unix_ms: null, cpu: "unavailable", cpu_cores: "unavailable",
  memory: "unavailable", disk_capacity: "unavailable", disk_io: "unavailable", network: "unavailable" };

test("an explicit telemetry request rejects an old backend snapshot with a stable error code", async () => {
  for (const metadata of [undefined, null]) {
    const api = loadApi(booted(metadata));
    await assert.rejects(api.bootstrapApp({ includeSystemSnapshot: true }), (error) => {
      assert.equal(JSON.parse(error.message).code, "system_telemetry_contract_missing");
      return true;
    });
    assert.deepEqual(api.calls, [{ command: "bootstrap", args: {
      includeSystemSnapshot: true, include_system_snapshot: true
    } }]);
  }
});

test("lightweight startup can still load inventory before a telemetry request", async () => {
  const value = booted();
  const api = loadApi(value);
  assert.equal(await api.bootstrapApp(), value);
  assert.deepEqual(api.calls[0].args, { includeSystemSnapshot: false, include_system_snapshot: false });
});

test("new-backend cold, warming, unavailable and measured samples retain their quality unchanged", async () => {
  for (const metadata of [cold,
    { ...cold, observed_at_unix_ms: Date.now(), cpu: "warming_up", network: "warming_up" },
    { ...cold, observed_at_unix_ms: Date.now() },
    Object.fromEntries(Object.keys(cold).map((key) => [key, key === "observed_at_unix_ms" ? Date.now() : "valid"]))
  ]) {
    const value = booted(metadata);
    assert.equal(await loadApi(value).bootstrapApp({ includeSystemSnapshot: true }), value);
    assert.equal(value.state.snapshot.telemetry, metadata);
  }
});

test("the existing home refresh feedback localizes the backend mismatch in both languages", () => {
  const file = path.join(__dirname, "../src/App.tsx");
  const source = fs.readFileSync(file, "utf8");
  const start = source.indexOf("const handleSystemPollingError =");
  const end = source.indexOf("const handleRuntimeInstancesSynced =", start);
  assert.ok(start >= 0 && end > start);
  const expected = {
    "zh-CN": "后台版本未更新，请完整退出并重新启动应用。",
    "en-US": "The backend is out of date. Fully quit and restart the app."
  };
  for (const locale of Object.keys(expected)) {
    let issue = null;
    const handler = vm.runInNewContext(transpileTypeScript(source.slice(start, end) + "\nhandleSystemPollingError;", file), {
      useEffectEvent: (callback) => callback,
      retirement: { isPending: () => false, currentRevision: () => 0 },
      setSystemRefreshIssue: (update) => { issue = update(issue); },
      describeError: (error) => error.message, formatDesktopError, t: translator(locale)
    });
    handler(new Error(JSON.stringify({ code: "system_telemetry_contract_missing", message: "System telemetry metadata is missing." })), 0);
    assert.equal(issue.message, expected[locale]);
    assert.equal(issue.consecutiveFailures, 1);
    handler(new Error("unrelated native diagnostic"), 0);
    assert.equal(issue.message, "unrelated native diagnostic");
  }
});
