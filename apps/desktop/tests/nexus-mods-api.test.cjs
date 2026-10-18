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
    () => api.installNexusModFiles("instance", [{ reference: "123", file_id: "456" }])]) {
    await assert.rejects(operation(), (error) => error.code === "nexus_local_desktop_required");
  }
  assert.equal(calls, 0);
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
