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
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": {},
    "../instance-panel-refresh": {},
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
