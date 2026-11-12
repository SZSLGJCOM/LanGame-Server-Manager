const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
function load(invoke, local = true) {
  const filename = path.resolve(__dirname, "../src/nexus-mods-api.ts"), module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports, Error, URL,
    require(name) { assert.equal(name, "./api-transport"); return { invokeOrMock: invoke, canUseLocalDesktopApi: () => local }; } });
  return module.exports;
}

test("Nexus API controls stay unavailable and native refusal is preserved", async () => {
  const api = load(async () => { throw JSON.stringify({ code: "nexus_api_unavailable" }); });
  assert.equal(api.canUseNexusApi(), false);
  assert.equal(load(async () => {}, false).canUseNexusApi(), false);
  await assert.rejects(api.installNexusModFiles("instance", [{ reference: "123", file_id: "456" }]),
    (error) => error.code === "nexus_api_unavailable");
});
test("every Nexus operation refuses LAN before sending a key, reference or request", async () => {
  let calls = 0;
  const api = load(async () => { calls++; }, false);
  for (const operation of [() => api.readNexusAccountStatus(), () => api.storeNexusApiKey("synthetic-key"), () => api.clearNexusApiKey(),
    () => api.reviewNexusModReferences("instance", ["nxm://game/mods/123/files/456?key=private"]),
    () => api.installNexusModFiles("instance", [{ reference: "123", file_id: "456" }]),
    () => api.readNexusWebsiteAuthState(),
    () => api.requestNexusCommand("open_nexus_login_host", { bounds: null }),
    () => api.requestNexusCommand("update_nexus_login_host", { sessionId: "fixture-login", bounds: null }),
    () => api.requestNexusCommand("close_nexus_login_host", { sessionId: "fixture-login" })]) {
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
  let networkCalls = 0;
  const lan = load(async () => { networkCalls++; }, false);
  await assert.rejects(lan.requestNexusCommand("install_nexus_website_mod", { reference: "https://www.nexusmods.com/7daystodie/mods/123" }),
    (error) => error.code === "nexus_local_desktop_required");
  assert.equal(networkCalls, 0);
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
    assert.equal(api.canUseNexusApi(), false);
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
test("Nexus errors expose only allowlisted codes, never private keys or signed links", async () => {
  for (const rejection of ["synthetic-private-key", JSON.stringify({ code: "unknown", message: "nxm://private?key=secret" }),
    JSON.stringify({ code: "nexus_auth_invalid", message: "synthetic-private-key" })]) {
    const api = load(async () => { throw rejection; });
    await assert.rejects(api.storeNexusApiKey("synthetic-private-key"), (error) => {
      assert.doesNotMatch(String(error), /private|secret|nxm/);
      assert.ok(["nexus_operation_failed", "nexus_auth_invalid"].includes(error.code)); return true;
    });
  }
});
test("explicitly safe staging errors retain actionable file-conflict context", async () => {
  const message = "Existing unowned Mods/Fixture/config.xml would be overwritten";
  const api = load(async () => { throw JSON.stringify({ code: "nexus_installation_failed", message }); });
  await assert.rejects(api.installNexusModFiles("instance", [{ reference: "123", file_id: "42" }]),
    (error) => error.code === "nexus_installation_failed" && error.safeMessage === message && error.message === "nexus_installation_failed");
});
test("website preparation errors retain the local failure reason", async () => {
  const message = "Install the server files before staging local mods";
  const api = load(async () => { throw JSON.stringify({ code: "nexus_preparation_failed", message }); });
  await assert.rejects(api.requestNexusCommand("install_nexus_website_mod"),
    (error) => error.code === "nexus_preparation_failed" && error.safeMessage === message);
});
test("a missing native command reports an outdated desktop host", async () => {
  for (const message of ["Command open_nexus_website_host not found", "unknown LanGame command: open_nexus_website_host"]) {
    const api = load(async () => { throw message; });
    await assert.rejects(api.requestNexusCommand("open_nexus_website_host"),
      (error) => error.code === "nexus_desktop_update_required");
  }
});
test("Nexus review validates the reference-index permutation and every selectable file", async () => {
  const file = { file_id: "42", name: "Server", file_name: "server.zip", version: "1", category: "MAIN", size_bytes: 12,
    description: "Server files", downloadable: true, unavailable_reason: null };
  const review = (index) => ({ reference_index: index, game_domain: "7daystodie", mod_id: "123", mod_name: "Fixture",
    page_url: "https://www.nexusmods.com/7daystodie/mods/123", files: [file], selected_file_id: null });
  const valid = load(async () => [review(1), review(0)]);
  assert.equal((await valid.reviewNexusModReferences("instance", ["123", "456"]))[0].reference_index, 1);
  for (const response of [[review(0), review(0)], [review(0), review(2)], [{ ...review(0), files: [{ ...file, downloadable: "yes" }] }],
    [{ ...review(0), files: [file, file] }], [{ ...review(0), page_url: "javascript:alert(1)" }], [{ ...review(0), page_url: "https://nexusmods.com.attacker.invalid/mods/123" }]]) {
    await assert.rejects(load(async () => response).reviewNexusModReferences("instance", response.length === 2 ? ["123", "456"] : ["123"]),
      (error) => error.code === "nexus_invalid_response");
  }
});
test("Nexus installation validates complete native staging results before reporting success", async () => {
  const item = { source_path: "fixture://source", target_path: "fixture://target", status: "copied", file_count: 2, total_bytes: 4 };
  const valid = { instance_id: "instance", module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods", target_path: "fixture://target",
    affected_root_names: ["Fixture"], items: [item], copied_file_count: 2, copied_total_bytes: 4 };
  assert.equal((await load(async () => valid).installNexusModFiles("instance", [])).items[0].file_count, 2);
  for (const invalid of [{ ...valid, source_label: undefined }, { ...valid, copied_total_bytes: -1 }, { ...valid, affected_root_names: [1] },
    { ...valid, items: [{ ...item, message: {} }] }, { ...valid, items: [{ ...item, total_bytes: Number.MAX_SAFE_INTEGER + 1 }] }]) {
    await assert.rejects(load(async () => invalid).installNexusModFiles("instance", []), (error) => error.code === "nexus_invalid_response");
  }
});
test("Nexus-specific references preserve short IDs, signed nxm case and encoded query bytes", () => {
  const filename = path.resolve(__dirname, "../src/views/servers/nexus-mod-reference.ts"), module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { module, exports: module.exports });
  const signed = "nxm://7daystodie/mods/123/files/456?key=aB%2FC&expires=123&user_id=789";
  assert.deepEqual(Array.from(module.exports.nexusReferencesFromText(`123\n${signed}\n123`)), ["123", signed]);
  assert.deepEqual(Array.from(module.exports.nexusReferencesFromText(signed.replaceAll("&", "&amp;"))), [signed]);
});
