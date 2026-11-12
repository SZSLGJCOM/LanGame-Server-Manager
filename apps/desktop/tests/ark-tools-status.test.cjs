const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readArkToolsStatus, prepareArkTools, removeArkTools } = require("../src/api-ark-tools.ts");
const { queuedArkToolsInstallation } = require("../src/ark-tools-installation.ts");
const current = { installed: true, updateAvailable: false, connected: false, canUninstall: true, issue: null };

function native(t) {
  const previousWindow = globalThis.window;
  const hadIsTauri = Object.hasOwn(globalThis, "isTauri"), previousIsTauri = globalThis.isTauri;
  const state = { response: current, calls: [] };
  globalThis.isTauri = true;
  globalThis.window = { isTauri: true, localStorage: { getItem: () => "zh-CN" }, navigator: { language: "zh-CN" },
    __TAURI_INTERNALS__: { invoke: async (command, args) => {
      state.calls.push({ command, args });
      return structuredClone(state.response);
    } } };
  t.after(() => {
    if (previousWindow === undefined) delete globalThis.window; else globalThis.window = previousWindow;
    if (hadIsTauri) globalThis.isTauri = previousIsTauri; else delete globalThis.isTauri;
  });
  return state;
}

test("ARK status keeps an intact previous bundle installed and exposes its available update", async t => {
  const state = native(t);
  for (const response of [current, { ...current, updateAvailable: true },
    { ...current, updateAvailable: true, connected: true },
    { ...current, updateAvailable: true, issue: "Native extension handshake failed" }]) {
    state.response = response;
    assert.deepEqual(await readArkToolsStatus("ark-a", "map-b"), response);
  }
  assert.deepEqual(state.calls[0], {
    command: "read_ark_tools_status", args: { input: { instanceId: "ark-a", processKey: "map-b" } }
  });
  state.response = { ...current, installed: false, canUninstall: false,
    issue: "Prepare the ARK server extension while this instance is stopped" };
  assert.deepEqual(await readArkToolsStatus("ark-a"), { ...state.response, issue: null });
});

test("ARK status rejects malformed or unverified available-update claims", async t => {
  const state = native(t);
  for (const change of [{ updateAvailable: undefined }, { updateAvailable: null }, { updateAvailable: "true" },
    { installed: false, updateAvailable: true }, { installed: false, connected: true }]) {
    state.response = { ...current, ...change };
    await assert.rejects(readArkToolsStatus("ark-a"), /Invalid ARK extension status/);
  }
});

test("ARK preparation and removal receipts preserve the verified update state", async t => {
  const state = native(t);
  state.response = { ...current, progress: { ...queuedArkToolsInstallation("ark-a", "update-a"), phase: "ready", active: false } };
  assert.deepEqual(await prepareArkTools("ark-a", "update-a"), state.response);
  assert.deepEqual(state.calls[0], { command: "prepare_ark_tools", args: { input: {
    instanceId: "ark-a", operationId: "update-a", locale: "zh-CN", allowMatchingSymbolsDownload: true
  } } });
  state.response = { ...current, installed: false, canUninstall: false, frameworkRetained: true };
  assert.deepEqual(await removeArkTools("ark-a"), state.response);
  state.response.updateAvailable = true;
  await assert.rejects(removeArkTools("ark-a"), /Invalid ARK extension status/);
});
