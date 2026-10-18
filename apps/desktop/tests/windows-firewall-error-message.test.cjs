const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
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

test("firewall cancellation preserves strict startup context without echoing native text", () => {
  const payload = { code: "windows_firewall_setup_failed", reason: "elevation_cancelled",
    message: "untrusted native detail https://private.invalid/?token=secret",
    instance_name: "ASA Main", local_address: "192.168.31.150", log_path: "D:/logs/app.log", continue_start: false };
  for (const locale of ["zh-CN", "en-US"]) {
    const result = formatDesktopError(translator(locale), JSON.stringify(payload));
    assert.match(result, /ASA Main/);
    assert.match(result, /192\.168\.31\.150/);
    assert.match(result, /D:\/logs\/app\.log/);
    assert.doesNotMatch(result, /private|token|secret|errors\.firewall/);
    assert.match(result, locale === "zh-CN" ? /授权已取消.*尚未启动服务器/s : /prompt was cancelled.*server was not started/s);
  }
});

test("firewall failures explain unconfirmed partial state and wildcard continuation", () => {
  for (const reason of ["verification_failed", "cleanup_failed", "timeout", "apply_failed"]) {
    const payload = { code: "windows_firewall_setup_failed", reason, message: "unused", continue_start: true };
    const english = formatDesktopError(translator("en-US"), payload);
    assert.match(english, /Some rules may have changed/);
    assert.match(english, /continues starting.*external connections may be blocked/);
    const chinese = formatDesktopError(translator("zh-CN"), payload);
    assert.match(chinese, /部分规则可能已改变/);
    assert.match(chinese, /继续启动.*外部连接可能受阻/);
  }
});

test("firewall localization preserves unrelated errors and malformed contracts", () => {
  for (const input of ["An unrelated native error", JSON.stringify({ code: "unrelated", message: "keep this" }),
    JSON.stringify({ code: "windows_firewall_setup_failed", reason: "unknown", message: "keep this" }),
    JSON.stringify({ code: "windows_firewall_setup_failed", reason: "permission_denied", message: "keep this", log_path: {} })]) {
    assert.equal(formatDesktopError(translator("en-US"), input), input);
  }
});
