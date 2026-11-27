const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
function load(invoke, local = true) {
  const filename = path.resolve(__dirname, "../src/nexus-mods-api.ts"), module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports, Error,
    require(name) { assert.equal(name, "./api-transport"); return { invokeOrMock: invoke, canUseLocalDesktopApi: () => local }; } });
  return module.exports;
}

test("every active Nexus operation refuses LAN before sending a request", async () => {
  let calls = 0;
  const api = load(async () => { calls++; }, false);
  for (const operation of [
    () => api.readNexusWebsiteAuthState(),
    () => api.requestNexusCommand("open_nexus_login_host", { bounds: null }),
    () => api.requestNexusCommand("update_nexus_login_host", { sessionId: "fixture-login", bounds: null }),
    () => api.requestNexusCommand("close_nexus_login_host", { sessionId: "fixture-login" }),
    () => api.requestNexusCommand("open_nexus_website_host"),
    () => api.requestNexusCommand("install_nexus_website_mod", { reference: "https://www.nexusmods.com/7daystodie/mods/123" })
  ]) {
    await assert.rejects(operation(), (error) => error.code === "nexus_local_desktop_required");
  }
  assert.equal(calls, 0);
});

test("website commands need only the local desktop and preserve safe website errors", async () => {
  let received;
  const local = load(async (command, args) => { received = { command, args }; return "website-session"; });
  assert.equal(await local.requestNexusCommand("open_nexus_website_host"), "website-session");
  assert.equal(received.command, "open_nexus_website_host");
  assert.equal(received.args, undefined, "Background sessions have no presentation bounds");
  for (const code of ["nexus_browser_cancelled", "nexus_browser_page_unavailable", "nexus_format_unsupported"]) {
    const failure = load(async () => { throw JSON.stringify({ code, message: "synthetic-private-download" }); });
    await assert.rejects(failure.requestNexusCommand("install_nexus_website_mod"),
      (error) => error.code === code && !String(error).includes("synthetic-private-download"));
  }
});

test("website authentication reads only the native observation enum without opening a page", async () => {
  for (const state of ["unknown", "signed_out", "signed_in"]) {
    const calls = [];
    const api = load(async (command, args) => { calls.push({ command, args }); return state; });
    assert.equal(await api.readNexusWebsiteAuthState(), state);
    assert.deepEqual(calls, [{ command: "read_nexus_website_auth_state", args: undefined }]);
  }
  for (const invalid of [undefined, true, "logged_in", { state: "signed_in" }]) {
    await assert.rejects(load(async () => invalid).readNexusWebsiteAuthState(), (error) => error.code === "nexus_invalid_response");
  }
});

test("background download blockers retain their cause without exposing website messages", async () => {
  for (const [code, zh, en] of [
    ["nexus_website_login_required", /要求登录/, /requires sign-in/],
    ["nexus_website_verification_required", /网站验证未完成/, /website verification did not finish/],
    ["nexus_file_selection_required", /无法唯一确定/, /unambiguously/],
    ["nexus_website_action_required", /人工操作/, /manual action/],
    ["nexus_website_controls_unavailable", /下载控件/, /Download controls/],
    ["nexus_website_file_quarantined", /被 Nexus 隔离/, /Nexus has quarantined this file/],
  ]) {
    const api = load(async () => { throw JSON.stringify({ code, message: "https://cf-files.nexusmods.com/private.zip?token=fixture-client-secret" }); });
    const filename = path.resolve(__dirname, "../src/views/servers/nexus-mods-messages.ts"), module = { exports: {} };
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports,
      require(name) { assert.equal(name, "../../nexus-mods-api"); return api; } });
    await assert.rejects(api.requestNexusCommand("install_nexus_website_mod"), (error) => {
      assert.equal(error.code, code);
      assert.equal(error.safeMessage, undefined);
      for (const [locale, reason] of [["zh-CN", zh], ["en", en]]) {
        const text = module.exports.nexusErrorText(locale, error);
        assert.match(text, reason);
        assert.match(text, code === "nexus_website_file_quarantined"
          ? locale === "zh-CN" ? /暂时无法下载/ : /cannot be downloaded right now/
          : locale === "zh-CN" ? /未完成/ : /did not complete/);
        assert.doesNotMatch(text, /fixture-client-secret|cf-files|token=|API Key/);
        if (code === "nexus_website_login_required") {
          assert.match(text, locale === "zh-CN" ? /商店.*Nexus 登录.*应用内.*关闭登录页.*重试/ : /Nexus sign-in in Store.*within LanGame.*close the page.*retry/);
          assert.doesNotMatch(text, /default browser|external browser|系统浏览器|已登录/);
        }
      }
      return true;
    });
  }
});

test("Nexus errors expose only allowlisted codes, never private tokens or signed links", async () => {
  for (const rejection of ["synthetic-private-key", JSON.stringify({ code: "unknown", message: "nxm://private?key=secret" }),
    JSON.stringify({ code: "nexus_auth_invalid", message: "synthetic-private-key" })]) {
    const api = load(async () => { throw rejection; });
    await assert.rejects(api.requestNexusCommand("install_nexus_website_mod"), (error) => {
      assert.doesNotMatch(String(error), /private|secret|nxm/);
      assert.ok(["nexus_operation_failed", "nexus_auth_invalid"].includes(error.code)); return true;
    });
  }
});

test("explicitly safe staging and preparation errors retain actionable local context", async () => {
  const installMessage = "Existing unowned Mods/Fixture/config.xml would be overwritten";
  const installApi = load(async () => { throw JSON.stringify({ code: "nexus_installation_failed", message: installMessage }); });
  await assert.rejects(installApi.requestNexusCommand("install_nexus_website_mod"),
    (error) => error.code === "nexus_installation_failed" && error.safeMessage === installMessage && error.message === "nexus_installation_failed");

  const prepMessage = "Install the server files before staging local mods";
  const prepApi = load(async () => { throw JSON.stringify({ code: "nexus_preparation_failed", message: prepMessage }); });
  await assert.rejects(prepApi.requestNexusCommand("install_nexus_website_mod"),
    (error) => error.code === "nexus_preparation_failed" && error.safeMessage === prepMessage);
});

test("a missing native command reports an outdated desktop host", async () => {
  for (const message of ["Command open_nexus_website_host not found", "unknown LanGame command: open_nexus_website_host"]) {
    const api = load(async () => { throw message; });
    await assert.rejects(api.requestNexusCommand("open_nexus_website_host"),
      (error) => error.code === "nexus_desktop_update_required");
  }
});

test("nexusStageResult validates complete native staging results before reporting success", () => {
  const api = load(async () => {});
  const item = { source_path: "fixture://source", target_path: "fixture://target", status: "copied", file_count: 2, total_bytes: 4 };
  const valid = { instance_id: "instance", module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods", target_path: "fixture://target",
    affected_root_names: ["Fixture"], items: [item], copied_file_count: 2, copied_total_bytes: 4 };
  assert.equal(api.nexusStageResult("instance", valid).items[0].file_count, 2);
  for (const invalid of [{ ...valid, instance_id: "other" }, { ...valid, source_label: undefined }, { ...valid, copied_total_bytes: -1 },
    { ...valid, affected_root_names: [1] }, { ...valid, items: [{ ...item, message: {} }] },
    { ...valid, items: [{ ...item, total_bytes: Number.MAX_SAFE_INTEGER + 1 }] }]) {
    assert.throws(() => api.nexusStageResult("instance", invalid), (error) => error.code === "nexus_invalid_response");
  }
});

test("Nexus-specific references preserve short IDs, signed nxm case and encoded query bytes", () => {
  const filename = path.resolve(__dirname, "../src/views/servers/nexus-mod-reference.ts"), module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports });
  const signed = "nxm://7daystodie/mods/123/files/456?key=aB%2FC&expires=123&user_id=789";
  assert.deepEqual(Array.from(module.exports.nexusReferencesFromText(`123\n${signed}\n123`)), ["123", signed]);
  assert.deepEqual(Array.from(module.exports.nexusReferencesFromText(signed.replaceAll("&", "&amp;"))), [signed]);
});
