const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const requestId = "6759c0df-5605-4dce-8b78-46d82e830172";
const fixtureKey = ["synthetic", "key"].join("-");
const settings = { enabled: true, provider: "openai-compatible", model: " fixture-model ",
  baseUrl: " https://provider.example/v1 ", apiKey: fixtureKey, apiKeyStored: false };
const plain = (value) => JSON.parse(JSON.stringify(value));
function response() {
  const passed = { status: "passed", diagnostic: null, latencyMs: 10 };
  return { requestId, provider: settings.provider, model: "fixture-model", endpointUrl: "https://provider.example/v1/chat/completions",
    chat: { ...passed }, toolCall: { ...passed }, toolReplay: { ...passed }, elapsedMs: 30, requestCount: 3, cancelled: false };
}
function load({ desktop = true, lan = false, value = response() } = {}) {
  const calls = [];
  const module = { exports: {} };
  const file = path.join(__dirname, "../src/api-assistant-connection.ts");
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(file, "utf8"), file), {
    module, exports: module.exports, URL,
    require(name) {
      if (name === "@tauri-apps/api/core") return { isTauri: () => desktop };
      if (name === "./api-transport") return {
        shouldUseLanApi: () => lan,
        invokeOrMock: async (command, args) => { calls.push({ command, args }); return value; }
      };
      throw new Error(`Unexpected dependency: ${name}`);
    }
  }, { filename: file });
  return { api: module.exports, calls };
}

test("connection checks send only the chosen protocol configuration and exact cancellation identity", async () => {
  for (const desktop of [true, false]) {
    const check = load({ desktop, lan: !desktop });
    assert.equal(check.api.assistantConnectionCheckAvailable(), true);
    assert.deepEqual(plain(await check.api.checkAssistantConnection(settings, requestId)), response());
    assert.deepEqual(plain(check.calls), [{ command: "assistant_check_connection", args: {
      input: { requestId, settings: { provider: settings.provider, model: "fixture-model", baseUrl: "https://provider.example/v1", apiKey: fixtureKey } }
    } }]);
  }
  const cancel = load({ value: true });
  await cancel.api.cancelAssistantConnectionCheck(requestId);
  assert.deepEqual(plain(cancel.calls), [{ command: "assistant_cancel_connection_check", args: { requestId } }]);
});

test("an unconnected development preview cannot report a successful model check", async () => {
  const { api, calls } = load({ desktop: false });
  assert.equal(api.assistantConnectionCheckAvailable(), false);
  await assert.rejects(() => api.checkAssistantConnection(settings, requestId), /desktop host/);
  await assert.rejects(() => api.cancelAssistantConnectionCheck(requestId), /desktop host/);
  assert.equal(calls.length, 0);
});

test("public check responses exclude provider envelopes and unknown nested fields", async () => {
  const value = response();
  value.apiKey = ["synthetic", "private", "field"].join("-");
  value.reasoning = "private-provider-envelope";
  value.chat.reasoning = "private-nested-envelope";
  const { api } = load({ value });
  assert.deepEqual(plain(await api.checkAssistantConnection(settings, requestId)), response());
});

test("malformed and mismatched model check responses fail at the API boundary", async () => {
  const mutations = [
    (v) => { v.requestId = "another-request"; },
    (v) => { v.provider = "ollama"; },
    (v) => { v.model = "another-model"; },
    (v) => {
      const endpoint = new URL("https://provider.example/v1");
      endpoint.username = "fixture-user";
      endpoint.password = fixtureKey;
      v.endpointUrl = endpoint.toString();
    },
    (v) => { v.endpointUrl = "https://provider.example/v1?key=secret"; },
    (v) => { v.endpointUrl = "file:///private"; },
    (v) => { v.chat.status = "unknown"; },
    (v) => { v.toolCall.latencyMs = -1; },
    (v) => { v.toolReplay.diagnostic = { payload: "provider text" }; },
    (v) => { v.elapsedMs = Infinity; },
    (v) => { v.requestCount = 4; },
    (v) => { v.cancelled = "false"; }
  ];
  for (const mutate of mutations) {
    const value = response();
    mutate(value);
    const { api } = load({ value });
    await assert.rejects(() => api.checkAssistantConnection(settings, requestId), /Invalid connection-check/);
  }
  const { api } = load({ value: { ok: true } });
  await assert.rejects(() => api.cancelAssistantConnectionCheck(requestId), /Invalid connection-check cancellation/);
});

test("partial capability and cancellation reports keep their separate stage outcomes", async () => {
  const value = response();
  value.toolCall = { status: "failed", diagnostic: "tool_not_called", latencyMs: 25 };
  value.toolReplay = { status: "skipped", diagnostic: "tool_call_not_ready", latencyMs: 0 };
  value.requestCount = 2;
  const { api } = load({ value });
  assert.deepEqual(plain(await api.checkAssistantConnection(settings, requestId)), value);
});
