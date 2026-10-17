const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".tsx"] = require.extensions[".ts"];
const { formatSteamCmdError } = require("../src/steamcmd-error-message.ts");
const { formatDesktopError } = require("../src/desktop-error-message.ts");
const { requiresSteamWorkshopAccount } = require("../src/views/servers/steam-workshop-access-error.ts");
const { message: uiMessage, resolveUiMessage } = require("../src/app-ui.ts");
const { translate } = require("../src/i18n.tsx");
const catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const message = "Original diagnostic must remain available.";
const samples = [
  ["steamcmd_not_ready", { path: "D:/steamcmd/steamcmd.exe" }, "errors.steamcmdNotReady", undefined],
  ["steamcmd_preparation_stalled", { timeout_seconds: 90, output_excerpt: "Connecting to Steam" }, "errors.steamcmdPreparationStalled", { seconds: 90 }],
  ["steamcmd_executable_missing", { path: "D:/java.exe" }, "errors.steamcmdExecutableMissing", { path: "D:/java.exe" }],
  ["installed_executable_missing", { module_id: "game", operation: "install", path: "D:/server.exe" }, "errors.installedExecutableMissing", { module: "game", path: "D:/server.exe" }],
  ["installation_verification_failed", { module_id: "game", operation: "validate", detail: "expected file missing" }, "errors.installationVerificationFailed", { module: "game" }],
  ["install_operation_timed_out", { operation: "game server install lifecycle change", timeout_seconds: 900 }, "errors.installOperationTimedOut", { seconds: 900 }],
  ["install_operation_timed_out", { operation: "SteamCMD preparation", timeout_seconds: 900, output_excerpt: "Downloading update" }, "errors.installOperationTimedOut", { seconds: 900 }],
  ["steamcmd_root_unmanaged", { path: "D:/steamcmd" }, "errors.steamcmdRootUnmanaged", { path: "D:/steamcmd" }],
  ["steamcmd_ownership_invalid", { path: "D:/steamcmd" }, "errors.steamcmdOwnershipInvalid", { path: "D:/steamcmd" }],
  ["module_install_source_missing", { module_id: "game" }, "errors.moduleInstallSourceMissing", { module: "game" }],
  ["module_install_spec_missing", { module_id: "game" }, "errors.moduleInstallSpecMissing", { module: "game" }],
  ["module_process_spec_missing", { module_id: "game" }, "errors.moduleProcessSpecMissing", { module: "game" }],
  ["steamcmd_command_failed", { output_excerpt: "ERROR! Failed to install app" }, "errors.steamcmdCommandFailed", undefined],
  ["steam_account_name_invalid", {}, "errors.steamAccountNameInvalid", undefined],
  ["steam_account_authentication_failed", { output_excerpt: "Steam login did not complete" }, "errors.steamAccountAuthenticationFailed", undefined],
  ["steam_account_interactive_unsupported", {}, "errors.steamAccountInteractiveUnsupported", undefined],
  ["steam_app_account_required", { app_id: 896660, output_excerpt: "No subscription" }, "errors.steamAppAccountRequired", undefined],
  ["steamcmd_prepare_failed", { output_excerpt: "network failure" }, "errors.steamcmdPrepareFailed", undefined],
  ["module_download_failed", { output_excerpt: "extract failed" }, "errors.moduleDownloadFailed", undefined]
];

test("known installer failures choose localized messages without translating diagnostic output", () => {
  for (const [code, fields, expectedKey, expectedParams] of samples) {
    const error = { code, message, output_excerpt: null, ...fields };
    const original = JSON.stringify(error);
    const translate = (key, params) => {
      if (key === "errors.originalDiagnostic") return "Original diagnostic";
      assert.equal(key, expectedKey);
      assert.deepEqual(params, expectedParams);
      return "localized message";
    };
    const diagnostic = fields.detail ?? fields.output_excerpt;
    assert.equal(formatSteamCmdError(translate, error), diagnostic
      ? `localized message\nOriginal diagnostic\n${diagnostic}`
      : "localized message");
    assert.equal(JSON.stringify(error), original, "diagnostic evidence must not change");
  }
});

test("original command output remains readable when a SteamCMD failure has no background job", () => {
  const output = "ERROR (0x12): \"network unavailable\"\nD:\\steamcmd\\日志\r\n";
  const value = { code: "steamcmd_prepare_failed", message, output_excerpt: output };
  const rendered = formatSteamCmdError((key) => key === "errors.originalDiagnostic" ? "原始诊断" : "SteamCMD 下载或解压失败。", value);
  assert.equal(rendered, `SteamCMD 下载或解压失败。\n原始诊断\n${output}`);
});

test("a confirmed missing Workshop decryption key explains the local import path in both languages", () => {
  const output = "ERROR! Download item 3816899969 failed (Failure).\r\nWorkshop content: Missing decryption key\n";
  const payload = { code: "steam_workshop_decryption_key_missing", message, consumer_app_id: 1623730, output_excerpt: output };
  const raw = JSON.stringify(payload);
  const activity = uiMessage("servers.mods.referenceInstallFailed", { message: raw });
  for (const locale of ["zh-CN", "en-US"]) {
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    for (const value of [raw, new Error(raw), payload]) {
      const rendered = formatDesktopError(t, value);
      assert.match(rendered, locale === "zh-CN" ? /Steam 未向本次会话.*解密密钥/ : /Steam did not provide.*decryption key.*this session/);
      assert.match(rendered, locale === "zh-CN" ? /有访问权限.*登录后重试.*订阅并下载 Mod 及依赖/ : /account that has access.*retry.*subscribe.*download.*dependencies/);
      assert.ok(rendered.includes("LanGameCMD"), "the login recovery points to the built-in terminal");
      assert.ok(rendered.includes("steamapps/workshop/content/1623730"));
      assert.ok(rendered.includes(t("servers.mods.mainTabs.config")), "the instruction identifies the tab containing local import");
      assert.ok(rendered.includes(t("servers.mods.detailTabs.mods")), "the instruction identifies the Mod list that receives local folders");
      assert.ok(!rendered.includes(t("servers.mods.manualDropTitle")), "the instruction does not refer to the removed permanent drop banner");
      assert.ok(rendered.endsWith(`${t("errors.originalDiagnostic")}\n${output}`), "the original log remains exact");
      assert.doesNotMatch(rendered, /\{appId\}|steam_workshop_decryption_key_missing/);
      assert.ok(resolveUiMessage(t, activity).includes(rendered), "persisted installation failures use the same localized contract");
    }
  }
  assert.equal(activity.params.message, raw, "language changes retain the saved diagnostic");
});

test("Workshop decryption-key guidance requires a valid application ID and diagnostic contract", () => {
  const payload = { code: "steam_workshop_decryption_key_missing", message, consumer_app_id: 1623730, output_excerpt: "Missing decryption key" };
  const t = () => assert.fail("invalid payload must retain its diagnostic");
  for (const consumer_app_id of [undefined, null, "1623730", [], {}, 0, -1, Infinity, NaN, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    const invalid = { ...payload, consumer_app_id };
    assert.equal(formatSteamCmdError(t, invalid), null);
    const raw = JSON.stringify(invalid);
    assert.equal(formatDesktopError(t, raw), raw);
  }
  for (const output_excerpt of [undefined, null, 42, []]) {
    assert.equal(formatSteamCmdError(t, { ...payload, output_excerpt }), null);
  }
  for (const message of [undefined, null, 42, []]) {
    assert.equal(formatSteamCmdError(t, { ...payload, message }), null);
  }
});

test("a generic Workshop Failure does not imply a missing decryption key", () => {
  const output = "ERROR! Download item 3816899969 failed (Failure).";
  const payload = { code: "steamcmd_command_failed", message, output_excerpt: output };
  for (const locale of ["zh-CN", "en-US"]) {
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    assert.equal(formatDesktopError(t, payload), `${t("errors.steamcmdCommandFailed")}\n${t("errors.originalDiagnostic")}\n${output}`);
  }
});

test("only verified Workshop access contracts request interactive account recovery", () => {
  for (const code of ["steam_workshop_decryption_key_missing", "steam_workshop_account_required"]) {
    const payload = { code, message, consumer_app_id: 1623730, output_excerpt: "Native confirmed access failure" };
    for (const value of [payload, JSON.stringify(payload), new Error(JSON.stringify(payload))]) {
      assert.equal(requiresSteamWorkshopAccount(value), true);
    }
    for (const consumer_app_id of [null, "1623730", 0, -1, 1.5, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
      assert.equal(requiresSteamWorkshopAccount({ ...payload, consumer_app_id }), false);
    }
    for (const field of ["message", "output_excerpt"]) for (const invalid of [undefined, null, 123, []]) {
      assert.equal(requiresSteamWorkshopAccount({ ...payload, [field]: invalid }), false);
    }
  }
  for (const error of ["Failure", "Missing decryption key", "No subscription", new Error("AccessDenied"),
    { code: "steamcmd_command_failed", message, output_excerpt: "Failure / AccessDenied / Missing decryption key" },
    { code: "steam_workshop_network_failed", message, output_excerpt: "AccessDenied", consumer_app_id: 1623730 },
    null, [], { code: ["steam_workshop_account_required"], message, consumer_app_id: 1623730, output_excerpt: "denied" }]) {
    assert.equal(requiresSteamWorkshopAccount(error), false);
  }
});

test("confirmed account access and cancelled recovery have localized outcomes", () => {
  const output = "Workshop download failed (NoSubscription).\r\n";
  const payload = { code: "steam_workshop_account_required", message, consumer_app_id: 1623730, output_excerpt: output };
  for (const locale of ["zh-CN", "en-US"]) {
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    assert.equal(formatDesktopError(t, payload), `${t("errors.steamWorkshopAccountRequired")}\n${t("errors.originalDiagnostic")}\n${output}`);
    assert.equal(formatDesktopError(t, JSON.stringify({ code: "steam_workshop_login_cancelled", message })), t("errors.steamWorkshopLoginCancelled"));
  }
  for (const invalid of [{ ...payload, consumer_app_id: 0 }, { ...payload, output_excerpt: null }, { ...payload, message: [] }]) {
    assert.equal(formatSteamCmdError(() => assert.fail("invalid contract must retain its diagnostic"), invalid), null);
  }
});

test("Steam account login errors render in both languages and retain native failure evidence", () => {
  for (const locale of ["zh-CN", "en-US"]) {
    const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
    for (const [code, key] of [["steam_account_name_invalid", "errors.steamAccountNameInvalid"],
      ["steam_account_interactive_unsupported", "errors.steamAccountInteractiveUnsupported"]]) {
      assert.equal(formatDesktopError(t, JSON.stringify({ code, message })), t(key));
    }
    const output = "Steam login failed (InvalidPassword).\r\n";
    const payload = { code: "steam_account_authentication_failed", message, output_excerpt: output };
    assert.equal(formatDesktopError(t, payload), `${t("errors.steamAccountAuthenticationFailed")}\n${t("errors.originalDiagnostic")}\n${output}`);
    for (const output_excerpt of [undefined, null, 123, []]) {
      const invalid = JSON.stringify({ ...payload, output_excerpt });
      assert.equal(formatDesktopError(t, invalid), invalid);
    }
  }
});

test("invalid parameter shapes and unknown failures fall back to the original diagnostic", () => {
  const translate = () => assert.fail("invalid payload must not be translated");
  for (const [code, fields] of samples) {
    assert.equal(formatSteamCmdError(translate, { code, ...fields, message: 42 }), null);
    assert.equal(formatSteamCmdError(translate, { code, ...fields, message, output_excerpt: 42 }), null);
    for (const field of Object.keys(fields)) {
      assert.equal(formatSteamCmdError(translate, { code, message, ...fields, [field]: [] }), null, `${code}.${field}`);
    }
  }
  for (const timeout_seconds of [-1, Infinity, NaN, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(formatSteamCmdError(translate, { code: "install_operation_timed_out", message, operation: "install", timeout_seconds }), null);
  }
  assert.equal(formatSteamCmdError(translate, { code: "os_error", message }), null);
});

test("module names and paths preserve quotes, newlines and Unicode", () => {
  const module = "游戏 \"A\"\\B\nC";
  const path = "D:/游戏/server \"A\".exe";
  const value = { code: "installed_executable_missing", message, module_id: module, path, operation: "install" };
  assert.equal(formatSteamCmdError((key, params) => {
    assert.equal(params.module, module);
    assert.equal(params.path, path);
    return "translated";
  }, value), "translated");
});
