const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { collectTranslationReferences } = require("../scripts/i18n-source-references.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;

const { localizeRuntimeHealthSummary } = require("../src/runtime-health-message.ts");
const { buildAssistantViewModel } = require("../src/assistant-state.ts");
const { buildMockRuntimeHealth } = require("../src/api-mock/runtime-health.ts");
const { translate } = require("../src/i18n.tsx");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const catalogs = { "en-US": EN_US_MESSAGES, "zh-CN": ZH_CN_MESSAGES };
const translator = (locale) => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);

function healthFor(code, summary, params = {}, status = "warning") {
  return { status, summary, reason: { code, params }, matched_line: "[12:00] RAW engine diagnostic" };
}

test("Abiotic listening and map loading remain startup states in the UI", () => {
  const details = { summary: { module_id: "abioticfactor", status: "Running" } };
  for (const line of ["Listening on port 7777", "Load map complete", "Dedicated server is now loading the main map"]) {
    const health = buildMockRuntimeHealth(details, { lines: [line] });
    assert.equal(health.status, "starting");
    for (const t of [undefined, translator("zh-CN")]) {
      assert.doesNotMatch(localizeRuntimeHealthSummary(health, "zh-CN", t), /已准备好|已就绪/);
    }
    assert.doesNotMatch(localizeRuntimeHealthSummary(health, "en-US", translator("en-US")), /looks ready/);
  }
  const ready = buildMockRuntimeHealth(details, {
    lines: ["Session short code: current-session", "Listening on port 7777"]
  });
  assert.equal(ready.status, "ready");
  assert.equal(ready.reason.code, "abiotic_session_published");
});

test("runtime health uses diagnostic reasons rather than status or English text matching", () => {
  const stopped = healthFor("stopped", "The stopped summary can change.", {}, "warning");
  const starting = healthFor("starting_waiting_logs", "Waiting.", {}, "warning");
  assert.equal(localizeRuntimeHealthSummary(stopped, "zh-CN"), "服务器当前已停止。");
  assert.equal(localizeRuntimeHealthSummary(starting, "zh-CN"), "进程正在运行，等待启动日志输出。");
  assert.equal(localizeRuntimeHealthSummary(stopped, "en-US"), stopped.summary);
  assert.equal(localizeRuntimeHealthSummary(healthFor("new_engine_reason", "Keep unknown diagnostic."), "zh-CN"), "Keep unknown diagnostic.");
  assert.equal(localizeRuntimeHealthSummary({ status: "error", summary: "No structured reason.", matched_line: null }, "zh-CN"), "No structured reason.");
});

test("runtime health translation preserves diagnostic parameters and interpolates only the template", () => {
  const error = "Access denied: D:/instances/中文/{error}.log";
  const health = healthFor("log_read_failed", `Runtime log could not be read: ${error}`, { error });
  for (const t of [undefined, translator("zh-CN"), (key) => key]) {
    const localized = localizeRuntimeHealthSummary(health, "zh-CN", t);
    assert.match(localized, /^无法读取运行日志：/);
    assert.ok(localized.endsWith(error));
  }
  assert.equal(localizeRuntimeHealthSummary(health, "en-US"), health.summary);
  assert.equal(localizeRuntimeHealthSummary(health, "en-US", (key) => key), health.summary);
  const ready = healthFor("dragonwilds_ready_map", "Ready on Test World.", { map: "Test World" });
  assert.match(localizeRuntimeHealthSummary(ready, "zh-CN", translator("zh-CN")), /地图 Test World/);
  const ports = healthFor("ark_udp_pending", "Waiting for ports.", { bound: "7777", missing: "7778, 27015" });
  assert.match(localizeRuntimeHealthSummary(ports, "zh-CN", translator("zh-CN")), /7777.*7778, 27015/);
});

test("every runtime health message has explicit English and Chinese catalog coverage", () => {
  const filename = path.join(__dirname, "../src/runtime-health-message.ts");
  const references = collectTranslationReferences(fs.readFileSync(filename, "utf8"), filename);
  assert.ok(references.size >= 21);
  for (const key of references.keys()) {
    assert.ok(EN_US_MESSAGES[key], `Missing English message: ${key}`);
    assert.ok(ZH_CN_MESSAGES[key], `Missing Chinese message: ${key}`);
    assert.notEqual(EN_US_MESSAGES[key], ZH_CN_MESSAGES[key], `Untranslated health message: ${key}`);
  }
});

function assistantInput(health) {
  return {
    aiSettings: { enabled: false, provider: "ollama", model: "", baseUrl: "", apiKey: "", apiKeyStored: false },
    locale: "zh-CN", activeJobsCount: 0, activeView: "servers", storageReady: true,
    bootstrap: { state: { storage: {}, snapshot: {}, modules: [], instances: [] } },
    libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
    selectedInstanceId: "selected", selectedInstanceModuleDetails: null, selectedModuleDetails: null,
    selectedInstanceDetails: { summary: { id: "selected", name: "Server", status: "Running", module_id: "test" }, ports: [] },
    selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null, steamCmdStatus: null,
    serverWorkspaceSection: "overview",
    selectedRuntime: { instance_id: "selected", health, recent_runs: [], log_tail: { source_path: null, lines: [health.matched_line] } }
  };
}

test("assistant issues and AI context use localized health while source diagnostics remain unchanged", () => {
  const health = healthFor("fatal_log_pattern", "A fatal runtime error pattern was detected in the recent log.", {}, "error");
  const input = assistantInput(health);
  const before = structuredClone(input);
  for (const locale of ["zh-CN", "en-US"]) {
    const model = buildAssistantViewModel({ ...input, locale }, translator(locale));
    const summary = localizeRuntimeHealthSummary(health, locale, translator(locale));
    assert.ok(model.issues.find((issue) => issue.id === "runtime-error").detail.includes(summary));
    assert.ok(model.contextPayload.includes(`Summary: ${summary}`));
    assert.ok(model.contextPayload.includes(`Matched Line: ${health.matched_line}`));
    assert.ok(model.prompts.find((prompt) => prompt.id === "logs").prompt.includes(summary));
  }
  assert.deepEqual(input, before);
});

test("mock health exposes the same reasons and raw log evidence to the assistant", () => {
  const raw = "[12:03] Failed to load modoverrides.lua";
  const health = buildMockRuntimeHealth({ summary: { module_id: "dontstarve", status: "Running" } }, { lines: [raw] });
  assert.equal(health.reason.code, "dst_lua_config_failed");
  assert.equal(health.matched_line, raw);
  assert.match(localizeRuntimeHealthSummary(health, "zh-CN"), /分片 Lua 配置文件加载失败/);
});

test("assistant prompt has a localized empty runtime summary", () => {
  for (const [locale, expected] of [["zh-CN", "暂无运行状态摘要。"], ["en-US", "No runtime health summary yet."]]) {
    const input = { ...assistantInput(healthFor("stopped", "Stopped")), locale, selectedRuntime: null };
    const prompt = buildAssistantViewModel(input).prompts.find((item) => item.id === "logs").prompt;
    assert.ok(prompt.includes(expected), prompt);
  }
});
