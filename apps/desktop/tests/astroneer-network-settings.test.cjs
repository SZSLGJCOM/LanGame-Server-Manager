const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { parseAstroneerAdvertisedAddress, readAstroneerAdvertisedAddress, readAstroneerConnectionMode, resolveAstroneerConnectionMode,
  astroneerRadminCandidates, astroneerJoinAddressCandidates, buildAstroneerJoinAddressPatch, astroneerNetworkDraftMatchesPersisted,
  astroneerNetworkSettingsIssue } = require("../src/astroneer-network-settings.ts");

const radmin = (address) => ({ address, kind: "overlay", family_name: "Radmin VPN", adapter_name: "Renamed adapter" });

test("ASTRONEER advertised IPv4 accepts LAN and overlay unicast addresses and rejects malformed records", () => {
  for (const address of ["1.1.1.1", "26.56.79.75", "10.1.2.3", "100.64.0.0", "100.127.255.255", "172.16.0.0", "172.31.255.255", "192.168.1.2", "100.63.255.255", "100.128.0.0", "172.15.255.255", "172.32.0.0",
    "192.0.0.9", "192.0.0.10", "192.0.1.1", "192.88.99.1", "192.88.99.3", "198.17.255.255", "198.20.0.0", "223.255.255.255"]) {
    assert.equal(parseAstroneerAdvertisedAddress(address), address);
    assert.equal(readAstroneerAdvertisedAddress(JSON.stringify({ public_ip: address })), address);
  }
  for (const address of ["0.1.2.3", "127.0.0.1", "169.254.1.1", "192.0.0.8", "192.0.0.11", "192.0.2.1", "192.88.99.2",
    "198.18.0.0", "198.19.255.255", "198.51.100.1", "203.0.113.1", "224.0.0.0", "255.255.255.255",
    "1.1.1.01", " 1.1.1.1", "1.1.1.1 ", "1.1.1.1\n", "1.1.1.1\r", "1.1.1.1\r\n", "1.1.1.1\u2028",
    "1.1.1", "256.1.1.1", "1.1.1.1:8777", "::1", "", null, 123]) {
    assert.equal(parseAstroneerAdvertisedAddress(address), "", String(address));
    assert.equal(readAstroneerAdvertisedAddress(JSON.stringify({ public_ip: address })), "", String(address));
  }
  for (const raw of ["bad", "null", "[]", '"1.1.1.1"', "{}", '{"public_ip":{}}']) assert.equal(readAstroneerAdvertisedAddress(raw), "");
});

test("legacy explicit addresses stay manual and missing modes never migrate an existing address", () => {
  const legacy = Object.freeze({ public_ip: "1.1.1.1", server_name: "Keep this" });
  assert.equal(resolveAstroneerConnectionMode(legacy), "manual");
  assert.equal(resolveAstroneerConnectionMode({}), "public");
  assert.equal(resolveAstroneerConnectionMode({ public_ip: " " }), "public");
  assert.equal(resolveAstroneerConnectionMode({ connection_mode: "auto", public_ip: "1.1.1.1" }), null);
  assert.equal(resolveAstroneerConnectionMode({ connection_mode: null }), null);
  assert.equal(readAstroneerConnectionMode('{"connection_mode":"radmin","public_ip":"26.1.2.3"}'), "radmin");
  assert.equal(readAstroneerConnectionMode('{"public_ip":"1.1.1.1"}'), "manual");
  for (const raw of ["bad", "null", "[]", '{"connection_mode":"unknown"}']) assert.equal(readAstroneerConnectionMode(raw), null);
  assert.deepEqual(legacy, { public_ip: "1.1.1.1", server_name: "Keep this" });
});

test("the shared address selector derives startup checks without a second mode choice", () => {
  const candidates = [radmin("26.1.2.3"), radmin("26.1.2.3"),
    { address: "100.80.1.2", kind: "overlay", family_name: "Tailscale", adapter_name: "Radmin VPN" },
    { address: "192.168.1.2", kind: "lan" }, { address: "1.1.1.1", kind: "public" },
    { address: "10.8.0.1", kind: "proxy" }, { address: "0.0.0.0", kind: "all" },
    { address: "fd00::1", kind: "overlay" }];
  assert.deepEqual(astroneerRadminCandidates(candidates), [candidates[0]]);
  assert.deepEqual(astroneerJoinAddressCandidates(candidates), [candidates[0], candidates[2], candidates[3], candidates[4]]);
  assert.deepEqual(buildAstroneerJoinAddressPatch("26.1.2.3", candidates), { connection_mode: "radmin", public_ip: "26.1.2.3" });
  for (const address of ["192.168.1.2", "100.80.1.2", "8.8.4.4"]) {
    assert.deepEqual(buildAstroneerJoinAddressPatch(address, candidates), { connection_mode: "manual", public_ip: address });
  }
  assert.deepEqual(buildAstroneerJoinAddressPatch(null, candidates), { connection_mode: "public", public_ip: "" });
  assert.deepEqual(buildAstroneerJoinAddressPatch("", candidates), { connection_mode: "manual", public_ip: "" });
  assert.deepEqual(buildAstroneerJoinAddressPatch("26.9.9.9", candidates, "26.9.9.9"), { connection_mode: "radmin", public_ip: "26.9.9.9" });
  assert.deepEqual(buildAstroneerJoinAddressPatch("1.1.1.1", candidates, "26.9.9.9"), { connection_mode: "manual", public_ip: "1.1.1.1" });
});

test("address validation blocks incomplete manual and Radmin drafts while retaining automatic public discovery", () => {
  assert.equal(astroneerNetworkSettingsIssue({ connection_mode: "public", public_ip: "" }), null);
  assert.equal(astroneerNetworkSettingsIssue({ connection_mode: "radmin", public_ip: "26.1.2.3" }), null);
  assert.deepEqual(astroneerNetworkSettingsIssue({ connection_mode: "manual", public_ip: " " }), { fieldKey: "public_ip", reason: "manualRequired" });
  assert.deepEqual(astroneerNetworkSettingsIssue({ connection_mode: "radmin", public_ip: "" }), { fieldKey: "public_ip", reason: "radminRequired" });
  assert.equal(astroneerNetworkSettingsIssue({ connection_mode: "manual", public_ip: "192.168.1.2" }), null);
  assert.deepEqual(astroneerNetworkSettingsIssue({ connection_mode: "manual", public_ip: "127.0.0.1" }), { fieldKey: "public_ip", reason: "invalidAddress" });
  assert.deepEqual(astroneerNetworkSettingsIssue({ public_ip: null }), { fieldKey: "public_ip", reason: "invalidAddress" });
  assert.deepEqual(astroneerNetworkSettingsIssue({ connection_mode: "unknown" }), { fieldKey: "public_ip", reason: "invalidMode" });
});

test("copy remains blocked until both the selected mode and advertised address are persisted", () => {
  const persisted = JSON.stringify({ connection_mode: "public", public_ip: "1.1.1.1", server_name: "Old" });
  assert.equal(astroneerNetworkDraftMatchesPersisted(persisted, { connection_mode: "public", public_ip: "1.1.1.1", server_name: "New" }), true);
  assert.equal(astroneerNetworkDraftMatchesPersisted(persisted, { connection_mode: "radmin", public_ip: "1.1.1.1" }), false);
  assert.equal(astroneerNetworkDraftMatchesPersisted(persisted, { connection_mode: "public", public_ip: "26.1.2.3" }), false);
  assert.equal(astroneerNetworkDraftMatchesPersisted('{"public_ip":"1.1.1.1"}', { connection_mode: "manual", public_ip: "1.1.1.1" }), false);
  assert.equal(astroneerNetworkDraftMatchesPersisted("bad", {}), false);
});
