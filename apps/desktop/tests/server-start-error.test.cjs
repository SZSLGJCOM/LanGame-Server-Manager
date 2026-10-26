const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compile(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".ts"] = compile;
require.extensions[".tsx"] = compile;
const errors = require("../src/server-start-error.ts");
const { resolveUiMessage } = require("../src/app-ui.ts");
const { translate } = require("../src/i18n.tsx");
const catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const issue = (state = "NotInstalled") => ({
  code: "module_not_ready", module_id: "sevendaystodie", module_name: "7 Days to Die",
  install_state: state, message: "Finish, repair, or rescan the game install before launching an instance."
});
const render = (locale, value) => resolveUiMessage((key, params, fallback) => translate(locale, key, params, fallback, catalogs), value);

test("a stale DST startup snapshot is translated without requesting another confirmation", () => {
  const activity = errors.serverStartFailureMessage(JSON.stringify({ code: "dst_world_start_changed", message: "changed" }));
  assert.match(render("zh-CN", activity), /启动准备期间配置或存档状态发生变化/);
  assert.match(render("en-US", activity), /configuration or save state changed while preparing startup/);
  assert.doesNotMatch(render("zh-CN", activity), /确认/);
});

test("invalid pending drafts are translated when starting outside the editor", () => {
  const error = Object.assign(new Error("pending settings draft is invalid"), { code: "instance_settings_draft_invalid" });
  assert.match(render("zh-CN", errors.serverStartFailureMessage(error)), /配置中有无效内容/);
});

test("Tauri strings and LAN Error messages carry the same validated installation error", () => {
  const payload = issue();
  for (const value of [JSON.stringify(payload), new Error(JSON.stringify(payload)), payload]) {
    assert.deepEqual(errors.readModuleNotReadyError(value), payload);
  }
});

for (const [state, text] of [
  ["NotInstalled", "尚未安装"], ["Incomplete", "不完整"], ["Corrupted", "已损坏"],
  ["Installing", "处理中"], ["Updating", "处理中"], ["Uninstalling", "处理中"], ["Unknown", "尚未就绪"]
]) {
  test(`${state} produces a Chinese business message that can switch to English after the failure`, () => {
    const activity = errors.serverStartFailureMessage(JSON.stringify(issue(state)));
    const chinese = render("zh-CN", activity);
    assert.ok(chinese.includes(text));
    assert.doesNotMatch(chinese, /Finish|current install state|NotInstalled|Corrupted|module_not_ready/);
    const english = render("en-US", activity);
    assert.match(english, /server files/);
    assert.doesNotMatch(english, /[\u4e00-\u9fff]/);
    assert.ok(chinese.includes("7 Days to Die"));
  });
}

test("malformed payloads and unrelated errors retain their actual diagnostic rather than a guessed translation", () => {
  for (const value of ["module is not ready to start", "{bad json", null, [], { ...issue(), code: "other" }, { ...issue(), install_state: 4 }]) {
    assert.equal(errors.readModuleNotReadyError(value), null);
  }
  const actual = new Error("Access denied: D:/servers/game.exe");
  assert.equal(errors.serverStartFailureMessage(actual).params.message, actual.message);
});

test("start rejection refreshes installation state, keeps the localized error and never reports success", async () => {
  const source = path.join(__dirname, "../src/hooks/useDesktopActions.ts");
  const activity = [];
  const log = [];
  let refreshed = false;
  let resolveRefresh;
  const refresh = new Promise((resolve) => { resolveRefresh = resolve; });
  const deps = {
    react: { useRef: (value) => ({ current: value }),
      useState: (initial) => [typeof initial === "function" ? initial() : initial, () => assert.fail("start must not mutate creation state")] },
    "../api": {
      startInstance: async () => { throw JSON.stringify(issue()); },
      logFrontendEvent: async (...args) => { log.push(args); }
    },
    "../app-state": { describeError: (error) => error instanceof Error ? error.message : String(error) },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "./useSteamCmdActions": { useSteamCmdActions: () => assert.fail("instance actions must not start library hooks") },
    "../installation-cancellation": {},
    "../steam-installation-account": {},
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": {},
    "../instance-panel-loader": {},
    "../server-start-error": errors,
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => {} }) }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(source, "utf8"), source), {
    exports, require: (id) => { assert.ok(Object.hasOwn(deps, id), id); return deps[id]; }
  });
  const actions = exports.useInstanceActions({
    setActivity: (value) => activity.push(value),
    refreshInstallationState: async () => { await refresh; refreshed = true; },
    reloadBootstrap: () => assert.fail("failed start cannot report success")
  });
  const outcome = actions.handleStartServer("server-7days");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(refreshed, false);
  assert.match(render("zh-CN", activity.at(-1)), /尚未安装/);
  resolveRefresh();
  assert.equal(await outcome, false);
  assert.equal(refreshed, true);
  assert.match(log[0][2], /module_not_ready/);
  assert.match(render("en-US", activity.at(-1)), /not installed/);
  assert.equal(activity.at(-1).tone, "error", "start rejection must stay visible alongside active downloads");
});

test("ASA query rejection stays localized and never reports startup success", async () => {
  const source = path.join(__dirname, "../src/hooks/useDesktopActions.ts");
  const payload = JSON.stringify({ code: "asa_curseforge_startup_failed", message: "Safe native fallback",
    stage: "query", reason: "server_unreachable", http_status: null, map_key: "main",
    cache_verified: true, native_process_exited: true });
  const activity = [];
  let started = 0;
  const deps = {
    react: { useRef: (value) => ({ current: value }),
      useState: (initial) => [typeof initial === "function" ? initial() : initial, () => assert.fail("start must not mutate creation state")] },
    "../api": { startInstance: async () => { started++; throw new Error(payload); }, logFrontendEvent: async () => {} },
    "../app-state": { describeError: (error) => error instanceof Error ? error.message : String(error) },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "./useSteamCmdActions": { useSteamCmdActions: () => assert.fail("instance actions must not start library hooks") },
    "../installation-cancellation": {}, "../steam-installation-account": {},
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": {}, "../instance-panel-loader": {}, "../server-start-error": errors,
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => {} }) }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(source, "utf8"), source), {
    exports, require: (id) => { assert.ok(Object.hasOwn(deps, id), id); return deps[id]; }
  });
  const actions = exports.useInstanceActions({ setActivity: (value) => activity.push(value),
    reloadBootstrap: () => assert.fail("failed start cannot report success"),
    refreshInstallationState: () => assert.fail("CurseForge network failure must not request program repair") });
  assert.equal(await actions.handleStartServer("asa-one"), false);
  assert.equal(started, 1);
  assert.equal(activity.at(-1).tone, "error");
  assert.equal(activity.at(-1).params.message, payload);
  assert.match(render("zh-CN", activity.at(-1)), /模组版本查询失败/);
  assert.match(render("en-US", activity.at(-1)), /Mod version query failed/i);
  assert.match(render("zh-CN", activity.at(-1)), /内容完整性已验证.*原生模组初始化失败/);
  assert.match(render("en-US", activity.at(-1)), /cache passed content integrity checks.*native Mod initialization failed/i);
  assert.match(render("zh-CN", activity.at(-1)), /进程已退出/);
  assert.match(render("en-US", activity.at(-1)), /process has exited/i);
  assert.equal(activity.at(-1).runtimeStartNotice, undefined);
});

for (const [code, provider, itemIds, activityKey] of [
  ["mod_update_using_cache", "steam_workshop", ["1234"], "activity.serverStartUsingCache"],
  ["mod_update_source_required", "thunderstore", ["Namespace-Package"], "activity.serverStartModSourceRequired"]
]) test(`manual startup publishes its actual ${code} result after refreshing server state`, async () => {
  const source = path.join(__dirname, "../src/hooks/useDesktopActions.ts");
  const warning = { code, provider, reason: code === "mod_update_source_required" ? "unknown" : "network",
    item_ids: itemIds, message: "https://native.invalid/?signature=PRIVATE_RESPONSE",
    log_path: "C:/fixture/start.log", map_key: null, http_status: null };
  const result = { summary: { id: "workshop-one", name: "Workshop server" }, run_id: 37, warnings: [warning] };
  const activity = [];
  const order = [];
  const deps = {
    react: { useRef: (value) => ({ current: value }),
      useState: (initial) => [typeof initial === "function" ? initial() : initial, () => assert.fail("start must not mutate creation state")] },
    "../api": { startInstance: async (...args) => { order.push(["start", ...args]); return result; }, logFrontendEvent: async () => {} },
    "../app-state": { describeError: (error) => error instanceof Error ? error.message : String(error) },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "./useSteamCmdActions": { useSteamCmdActions: () => assert.fail("instance actions must not start library hooks") },
    "../installation-cancellation": {}, "../steam-installation-account": {},
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": {}, "../instance-panel-loader": {}, "../server-start-error": errors,
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => { order.push(["flush"]); } }) }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(source, "utf8"), source), {
    exports, require: (id) => { assert.ok(Object.hasOwn(deps, id), id); return deps[id]; }
  });
  const actions = exports.useInstanceActions({ setActivity: (value) => { activity.push(value); order.push(["activity", value.key]); },
    reloadBootstrap: async () => { order.push(["refresh"]); },
    setSelectedInstanceId: () => {}, markRuntimeRefreshed: () => {},
    refreshInstallationState: () => assert.fail("a cached successful start must not request program repair") });
  assert.equal(await actions.handleStartServer("workshop-one"), true);
  assert.equal(activity.at(-1).tone, "warning");
  assert.equal(activity.at(-1).runtimeStartNotice.id, "manual-start:workshop-one:37");
  assert.deepEqual(activity.at(-1).runtimeStartNotice.warnings, [warning]);
  assert.equal(activity.at(-1).key, activityKey);
  assert.ok(order.findIndex((entry) => entry[0] === "refresh") < order.findIndex((entry) => entry[1] === activityKey));
  if (code === "mod_update_source_required") {
    assert.match(render("zh-CN", activity.at(-1)), /更新来源需要确认.*尚未检查最新版本/);
    assert.match(render("en-US", activity.at(-1)), /sources need confirmation.*latest versions were not checked/);
    assert.doesNotMatch(render("zh-CN", activity.at(-1)) + render("en-US", activity.at(-1)), /缓存|cached|failed|did not complete/);
  } else assert.match(render("zh-CN", activity.at(-1)), /缓存/);
  assert.doesNotMatch(render("en-US", activity.at(-1)), /PRIVATE_RESPONSE|native\.invalid/);
});
