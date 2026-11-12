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
const base = { code: "astroneer_network_preflight_failed", message: "Untrusted response 203.0.113.42 token=private" };

test("ASTRONEER address discovery failure describes each service without claiming no public entry point", () => {
  const input = JSON.stringify({ ...base, reason: "discovery_failed", failures: [
    { source: "ipify", reason: "connection" }, { source: "aws", reason: "timeout" }
  ] });
  for (const locale of ["en-US", "zh-CN"]) {
    const result = formatDesktopError(translator(locale), new Error(input));
    assert.match(result, /api\.ipify\.org/);
    assert.match(result, /checkip\.amazonaws\.com/);
    assert.match(result, locale === "zh-CN" ? /连接失败.*请求超时/s : /connection failed.*request timed out/s);
    assert.match(result, locale === "zh-CN" ? /尚未更新或启动服务器/ : /updates and startup have not begun/);
    assert.match(result, locale === "zh-CN" ? /查询失败不代表没有公网入口/ : /Lookup failure does not prove you lack a public entry point/);
    assert.doesNotMatch(result, /203\.0\.113\.42|token|private|errors\.astroneerNetwork/);
  }
});

test("ASTRONEER invalid addresses and HTTP or malformed responses remain distinct", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const t = translator(locale);
    assert.match(formatDesktopError(t, { ...base, reason: "invalid_public_ip" }),
      locale === "zh-CN" ? /玩家连接地址无效.*有效 IPv4/s : /player join address is invalid.*valid IPv4/s);
    for (const reason of ["http", "response", "request"]) {
      const result = formatDesktopError(t, { ...base, reason: "discovery_failed", failures: [
        { source: "ipify", reason, ...(reason === "http" ? { status: 503 } : {}) }
      ] });
      assert.match(result, reason === "http" ? /HTTP 503/ : reason === "response"
        ? locale === "zh-CN" ? /地址响应无效/ : /invalid address response/
        : locale === "zh-CN" ? /未能完成请求/ : /request could not be completed/);
      assert.doesNotMatch(result, /八秒|eight seconds|token=private/);
    }
  }
});

test("ASTRONEER address preparation failures recover through the shared address control without exposing backend payloads", () => {
  const reasons = {
    adapter_query_failed: [/无法读取.*网络地址.*刷新地址/, /Could not read network addresses.*Refresh addresses/s],
    invalid_connection_mode: [/地址来源无效.*玩家连接地址/, /address source is invalid.*player join address/s],
    manual_address_required: [/玩家连接 IPv4.*自动选择地址/, /player join IPv4.*automatically at startup/s],
    radmin_unavailable: [/开启 Radmin.*同一网络.*重新检测/, /Enable Radmin.*same network.*detect again/],
    radmin_address_unavailable: [/所选 Radmin 地址.*重新检测/, /selected Radmin address.*Detect addresses again/s],
    radmin_address_ambiguous: [/多个 Radmin 地址.*选择一个/, /More than one Radmin address.*Choose one/s]
  };
  for (const [reason, [zh, en]] of Object.entries(reasons)) {
    for (const locale of ["zh-CN", "en-US"]) {
      const result = formatDesktopError(translator(locale), { ...base, reason, failures: [] });
      assert.match(result, locale === "zh-CN" ? zh : en);
      assert.match(result, locale === "zh-CN" ? /尚未更新或启动服务器/ : /updates and startup have not begun/);
      assert.doesNotMatch(result, /203\.0\.113\.42|token|private|errors\.astroneerNetwork/);
    }
  }
});

test("ASTRONEER error localization retains unrelated and malformed error contracts", () => {
  const payloads = [
    { ...base, reason: "unknown" },
    { ...base, reason: "invalid_public_ip", failures: [{}] },
    { ...base, reason: "discovery_failed" },
    { ...base, reason: "discovery_failed", failures: [] },
    { ...base, reason: "discovery_failed", failures: [{ source: "unknown", reason: "timeout" }] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "unknown" }] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "http" }] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "http", status: 999 }] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "timeout", status: 503 }] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "timeout" }, { source: "ipify", reason: "response" }] },
    { ...base, reason: "discovery_failed", failures: [null] },
    { ...base, reason: "discovery_failed", failures: [{ source: "ipify", reason: "timeout" }, { source: "aws", reason: "timeout" }, {}] }
  ];
  for (const input of ["Unrelated error", JSON.stringify({ code: "unknown", message: "keep" }), ...payloads.map(JSON.stringify)]) {
    assert.equal(formatDesktopError(translator("zh-CN"), input), input);
  }
});
