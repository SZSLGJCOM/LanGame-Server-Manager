const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".tsx"] = require.extensions[".ts"] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const warnings = require("../src/runtime-start-warning.ts");
const { serverStartSuccessMessage } = require("../src/server-start-error.ts");
const { formatInstanceAutostartJobDetail } = require("../src/instance-autostart-job.ts");
const { translate } = require("../src/i18n.tsx");
const catalogs = { "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES,
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES };
const t = locale => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
const warning = { code: "mod_update_using_cache", provider: "steam_workshop", reason: "network",
  item_ids: ["1715971", "941450"], message: "https://service.invalid/?signature=DO_NOT_RENDER_SECRET",
  log_path: "C:/fixture/logs/start.log", map_key: "main", http_status: null };

test("native first-world warnings survive manual and background receipts without Mod fallback claims", () => {
  const entry = { code: "world_initialization_incomplete", provider: null, reason: null, item_ids: [],
    message: "PRIVATE_RESPONSE", log_path: "C:/fixture/logs/start.log", map_key: null, http_status: null };
  assert.deepEqual(warnings.readRuntimeStartWarnings([entry]), [entry]);
  for (const fields of [{ provider: "steam_workshop" }, { reason: "network" }, { item_ids: ["42"] },
    { map_key: "main" }, { http_status: 503 }, { log_path: "https://native.invalid" }]) {
    assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...entry, ...fields }]), []);
  }
  for (const locale of ["zh-CN", "en-US"]) {
    const rendered = warnings.formatRuntimeStartWarning(t(locale), entry, "ASTRONEER");
    assert.match(rendered.text, locale === "zh-CN" ? /正在运行.*初始化未完成/ : /running.*initialization did not complete/);
    assert.doesNotMatch(JSON.stringify(rendered), /PRIVATE_RESPONSE|cache|Mod|runtime\.startWarning/);
  }
  const result = serverStartSuccessMessage({ summary: { name: "ASTRONEER" }, run_id: 3, warnings: [entry] }, "astro");
  assert.equal(result.key, "activity.serverStartWorldInitializationIncomplete");
  assert.equal(result.tone, "warning");
  assert.deepEqual(result.runtimeStartNotice.warnings, [entry]);
  assert.equal(warnings.runtimeStartJobNotices([{ kind: "StartInstance", status: "Completed", id: "auto", label: "Autostart ASTRONEER", warnings: [entry] }]).length, 1);
});

test("cache fallback warnings are bilingual, scoped, and never echo backend response bodies", () => {
  for (const provider of ["steam_workshop", "curse_forge"]) {
    for (const locale of ["zh-CN", "en-US"]) {
      const rendered = warnings.formatRuntimeStartWarning(t(locale), { ...warning, provider }, "Test server");
      assert.match(rendered.text, locale === "zh-CN" ? /尚未确认最新版本/ : /latest versions were not confirmed/);
      assert.match(rendered.details, locale === "zh-CN" ? /DNS.*尚未|尚未确认.*DNS/ : /specific DNS.*not confirmed/);
      assert.ok(rendered.details.includes(warning.log_path));
      assert.ok(rendered.details.includes("941450"));
      assert.doesNotMatch(JSON.stringify(rendered), /DO_NOT_RENDER_SECRET|service\.invalid|mod_update_using_cache/);
    }
  }
  for (const reason of ["authentication", "access_denied", "request", "not_found", "expired", "response",
    "http", "download", "install", "unknown"]) {
    const rendered = warnings.formatRuntimeStartWarning(t("zh-CN"), { ...warning, reason, http_status: reason === "http" ? 503 : null }, "Test");
    assert.doesNotMatch(rendered.details, /runtime\.startWarning/);
    if (reason === "http") assert.ok(rendered.details.includes("503"));
  }
});

test("unrecognized warning shapes cannot inject diagnostic bodies or fake cached startup evidence", () => {
  assert.deepEqual(warnings.readRuntimeStartWarnings([warning]), [warning]);
  for (const fields of [{ code: "other" }, { reason: "cancelled" }, { provider: "other" },
    { item_ids: ["0"] }, { item_ids: ["18446744073709551616"] }, { item_ids: ["token=secret"] },
    { log_path: "https://service.invalid/?signature=secret" }, { map_key: "main\nsecret" },
    { http_status: 99 }, { http_status: "503" }]) {
    assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...warning, ...fields }]), []);
  }
  assert.deepEqual(warnings.readRuntimeStartWarnings(undefined), []);
  assert.deepEqual(warnings.readRuntimeStartWarnings({ warnings: [warning] }), []);
});

test("Workshop warning IDs preserve the full unsigned 64-bit contract while CurseForge stays signed", () => {
  for (const itemId of ["1", "9223372036854775807", "9223372036854775808", "18446744073709551615"]) {
    const entry = { ...warning, item_ids: [itemId] };
    assert.deepEqual(warnings.readRuntimeStartWarnings([entry]), [entry], `valid Workshop ID ${itemId}`);
    const result = { summary: { name: "Workshop server" }, run_id: 1, warnings: [entry] };
    assert.equal(serverStartSuccessMessage(result, "workshop-one").tone, "warning", "manual receipt must retain the warning");
  }
  const curseForge = { ...warning, provider: "curse_forge", item_ids: ["9223372036854775807"] };
  assert.deepEqual(warnings.readRuntimeStartWarnings([curseForge]), [curseForge]);
  assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...curseForge, item_ids: ["9223372036854775808"] }]), []);
  for (const provider of ["steam_workshop", "curse_forge"]) {
    for (const itemId of ["18446744073709551616", "184467440737095516150", "0", "01", "-1", "+1", "1e19", " 1"]) {
      assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...warning, provider, item_ids: [itemId] }]), [],
        `${provider} must reject noncanonical or overflowing ID ${itemId}`);
    }
  }
});

test("Thunderstore warning IDs use the canonical namespace-package reference contract", () => {
  const entry = { ...warning, provider: "thunderstore", item_ids: ["Namespace-Package"] };
  for (const id of ["Namespace-Package", "Owner_1-Package_2", "_Owner-_Package", `${"A".repeat(128)}-${"b".repeat(128)}`]) {
    const candidate = { ...entry, item_ids: [id] };
    assert.deepEqual(warnings.readRuntimeStartWarnings([candidate]), [candidate]);
  }
  for (const id of ["123456", "Namespace", "-Package", "Namespace-", "Namespace-Package-1.2.3", "Name-Pkg-extra",
    "https://thunderstore.io/package/Name/Pkg/", "Name/Pkg", "Name-Pkg?token=secret", "Name-Pkg\n", "所有者-Package",
    `${"A".repeat(129)}-Package`, `Owner-${"A".repeat(129)}`]) {
    assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...entry, item_ids: [id] }]), []);
  }
  for (const locale of ["zh-CN", "en-US"]) {
    const rendered = warnings.formatRuntimeStartWarning(t(locale), entry, "Server A");
    assert.match(rendered.text, /Thunderstore/);
    assert.match(rendered.text, locale === "zh-CN" ? /尚未确认最新版本/ : /latest versions were not confirmed/);
    assert.doesNotMatch(JSON.stringify(rendered), /runtime\.startWarning|DO_NOT_RENDER_SECRET|service\.invalid/);
  }
});

test("source-required notices preserve installed content without claiming an update attempt or cache proof", () => {
  for (const reason of ["unknown", "request"]) {
    const entry = { ...warning, code: "mod_update_source_required", provider: "thunderstore", reason,
      item_ids: ["Namespace-Package"] };
    assert.deepEqual(warnings.readRuntimeStartWarnings([entry]), [entry]);
    assert.deepEqual(warnings.readRuntimeStartWarnings([{ ...entry, item_ids: [] }]), [{ ...entry, item_ids: [] }],
      "unknown original roots must not fabricate a package identity");
    for (const locale of ["zh-CN", "en-US"]) {
      const rendered = warnings.formatRuntimeStartWarning(t(locale), entry, "Server A");
      assert.match(rendered.text, locale === "zh-CN" ? /来源.*确认.*尚未检查最新版本/ : /source.*confirmation.*latest versions were not checked/i);
      assert.match(rendered.details, locale === "zh-CN" ? /来源.*固定版本.*请求更新/ : /source.*pinned version.*requesting an update/i);
      assert.ok(rendered.details.includes("Namespace-Package"));
      assert.doesNotMatch(JSON.stringify(rendered), /更新未完成|缓存核验|更新请求未成功|模组服务拒绝|失败原因|did not complete|cache checks|request failed|service rejected|specific cause|DO_NOT_RENDER_SECRET|service\.invalid/);
    }
    const result = { summary: { name: "Server A" }, run_id: 42, warnings: [entry] };
    const manual = serverStartSuccessMessage(result, "instance-a");
    assert.equal(manual.key, "activity.serverStartModSourceRequired");
    assert.equal(manual.tone, "warning");
    assert.deepEqual(manual.runtimeStartNotice.warnings, [entry]);
    const mixed = serverStartSuccessMessage({ ...result, warnings: [warning, entry] }, "instance-a");
    assert.equal(mixed.key, "activity.serverStartModSourceRequired");
    assert.equal(mixed.runtimeStartNotice.warnings.length, 2);
    const job = { id: "source-required-one", kind: "StartInstance", status: "Completed", target_id: "instance-a",
      label: "Autostart Server A", warnings: [entry] };
    assert.equal(warnings.runtimeStartJobNotices([job])[0].id, "start-job:source-required-one");
    assert.deepEqual(warnings.runtimeStartJobNotices([JSON.parse(JSON.stringify(job))]), warnings.runtimeStartJobNotices([job]));
    assert.equal(warnings.runtimeStartJobNotices([{ ...job, id: "restart-two", label: "Restart Server A" }])[0].id, "start-job:restart-two");
    const detail = formatInstanceAutostartJobDetail(job, t("zh-CN"));
    assert.match(detail, /尚未检查最新版本/);
    assert.doesNotMatch(detail, /更新未完成|缓存核验|DO_NOT_RENDER_SECRET/);
    assert.notEqual(warnings.runtimeStartWarningKey(entry), warnings.runtimeStartWarningKey({ ...entry, code: "mod_update_using_cache" }));
    for (const provider of ["steam_workshop", "curse_forge"]) {
      const numeric = { ...entry, provider, item_ids: ["123456"] };
      assert.deepEqual(warnings.readRuntimeStartWarnings([numeric]), [numeric]);
    }
  }
});

test("manual startup keeps the returned warning without altering normal successful starts", () => {
  const result = { summary: { id: "instance-a", name: "Server A" }, run_id: 42, warnings: [warning] };
  const notice = serverStartSuccessMessage(result, "instance-a");
  assert.equal(notice.tone, "warning");
  assert.equal(notice.runtimeStartNotice.id, "manual-start:instance-a:42");
  assert.deepEqual(notice.runtimeStartNotice.warnings, [warning]);
  assert.equal(serverStartSuccessMessage({ ...result, warnings: [] }, "instance-a").key, "activity.serverStartSent");
  assert.equal(serverStartSuccessMessage(undefined, "instance-a").key, "activity.serverStartSent");
});

test("completed autostarts and restarts use stable job identities; failures and downloads are not successful starts", () => {
  const job = { id: "start-one", kind: "StartInstance", status: "Completed", target_id: "instance-a",
    label: "Autostart Server A", detail: "Started automatically with LGSM.", warnings: [warning] };
  const first = warnings.runtimeStartJobNotices([job]);
  assert.equal(first[0].id, "start-job:start-one");
  assert.deepEqual(warnings.runtimeStartJobNotices([JSON.parse(JSON.stringify(job))]), first);
  assert.equal(warnings.runtimeStartJobNotices([{ ...job, id: "restart-two", label: "Restart Server A" }])[0].id, "start-job:restart-two");
  for (const changed of [{ status: "Failed" }, { status: "Running" }, { status: "Cancelled" }, { kind: "DownloadWorkshop" }]) {
    assert.deepEqual(warnings.runtimeStartJobNotices([{ ...job, ...changed }]), []);
  }
  const detail = formatInstanceAutostartJobDetail(job, t("zh-CN"));
  assert.match(detail, /尚未确认最新版本/);
  assert.doesNotMatch(detail, /Started automatically|DO_NOT_RENDER_SECRET/);
  assert.equal(warnings.runtimeStartWarningKey(warning), warnings.runtimeStartWarningKey({ ...warning,
    item_ids: ["941450", "1715971", "941450"], message: "Changed backend wording" }));
});
