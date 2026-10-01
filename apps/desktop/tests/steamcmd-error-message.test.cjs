const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { formatSteamCmdError } = require("../src/steamcmd-error-message.ts");
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
