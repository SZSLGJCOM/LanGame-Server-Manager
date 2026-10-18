const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
const { translate } = require("../src/i18n.tsx");
const catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const { buildWorkshopVerificationWarning, buildWorkshopLocalizationWarning } = require("../src/views/servers/workshop-warning.ts");
const translator = (locale) => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
const failure = (overrides = {}) => JSON.stringify({ code: "steam_workshop_network_failed", stage: "item_type",
  message: "Steam temporarily refused the request", origin: "https://steamcommunity.com", reason: "http", status: 429,
  retry_after_seconds: 60, ...overrides });
const item = (id, message = failure()) => ({ id, status: "unverified", message });

test("Workshop warning groups affected Mods into a stable concise state while preserving diagnostics", () => {
  for (const locale of ["zh-CN", "en-US"]) {
    const t = translator(locale), first = buildWorkshopVerificationWarning(t, [item("111111")]);
    const next = buildWorkshopVerificationWarning(t, [item("111111", failure({ retry_after_seconds: 59 })), item("222222")]);
    assert.equal(first.summary, next.summary, "A cooldown or item-count change must not reopen a dismissed warning");
    assert.ok(first.summary.length < 90);
    assert.match(first.summary, locale === "zh-CN" ? /Steam.*限流.*无法下载.*重试/ : /Steam.*limiting.*cannot be downloaded.*Retry/);
    assert.doesNotMatch(first.summary, /HTTP|https:|60|111111|item_type/);
    assert.match(next.details, /111111/);
    assert.match(next.details, /222222/);
    assert.ok(next.details.includes(failure({ retry_after_seconds: 59 })));
    assert.match(next.details, /HTTP 429/);
    assert.equal(buildWorkshopVerificationWarning(t, [{ ...item("111111"), status: "resolved" }]), null);
  }
});

test("a single shared rate limit also explains original text without creating a second warning", () => {
  for (const locale of ["zh-CN", "en-US"]) {
    const t = translator(locale), localized = failure({ stage: "details", origin: "https://steamcommunity-a.akamaihd.net" });
    const result = buildWorkshopVerificationWarning(t, [item("111111"), item("222222")], {
      ...item("222222"), localization_warning: localized
    });
    assert.equal(result.includesOriginalText, true);
    assert.ok(result.summary.includes(t("servers.mods.warning.originalText")));
    assert.ok(result.details.includes(localized));
    assert.equal(buildWorkshopVerificationWarning(t, [item("111111")], {
      ...item("222222"), localization_warning: failure({ reason: "timeout", status: null })
    }).includesOriginalText, false);
  }
});

test("unknown diagnostics remain available without inventing Steam throttling", () => {
  const t = translator("zh-CN");
  for (const error of ["Unexpected HTTP 429 text", failure({ code: "other_error" }), failure({ origin: "https://unverified.example" })]) {
    const result = buildWorkshopVerificationWarning(t, [item("111111", error)]);
    assert.equal(result.summary, t("servers.mods.warning.unverified"));
    assert.ok(result.details.includes(error));
  }
  const error = "Unrecognized Workshop response";
  const result = buildWorkshopLocalizationWarning(t, error);
  assert.equal(result.summary, t("servers.mods.storeDetail.originalTextFallback"));
  assert.equal(result.details, error);
  assert.equal(buildWorkshopVerificationWarning(t, [item("111111", error)], {
    ...item("111111", error), localization_warning: error
  }).includesOriginalText, true);
});
