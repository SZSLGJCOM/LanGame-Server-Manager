const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const { resolveInstanceConnection, readPreferredJoinAddress } = require("../src/instance-connections.ts");
const { buildShareEndpoints, buildJoinEndpoint, buildInviteText, valheimCrossplayJoinGuidance } = require("../src/view-models.ts");
const t = (key, _params, fallback) => fallback ?? key;
const candidates = [
  { address: "0.0.0.0", kind: "all" },
  { address: "192.0.2.42", kind: "lan" },
  { address: "198.51.100.4", kind: "overlay" },
  { address: "203.0.113.8", kind: "public" },
  { address: "198.18.0.1", kind: "proxy" }
];
const details = (overrides = {}) => ({
  summary: { id: "fixture-server", module_id: "valheim", bind_ip: "0.0.0.0", port_count: 2 },
  ports: [{ name: "query", protocol: "udp", port: 28001 }, { name: "game", protocol: "udp", port: 28000 }],
  settings_json: "{}",
  ...overrides
});
const resolve = (value, addresses = candidates, preference = null) =>
  resolveInstanceConnection(value, addresses, preference, "zh-CN", t);

test("card connection uses the saved game port, not its count, query port or catalog default", () => {
  assert.equal(resolve(details()).endpoint, "203.0.113.8:28000");
  const changed = details({ ports: [{ name: "query", protocol: "udp", port: 29001 }, { name: "game", protocol: "udp", port: 29000 }] });
  assert.equal(resolve(changed).endpoint, "203.0.113.8:29000");
  assert.equal(resolve(details({ summary: { id: "dst", module_id: "dontstarve", bind_ip: "192.0.2.55" },
    ports: [{ name: "caves", protocol: "udp", port: 10999 }, { name: "master", protocol: "udp", port: 10998 }] })).endpoint,
  "192.0.2.55:10998");
});

test("card connection preserves preferred join address and falls back when that interface disappears", () => {
  assert.equal(resolve(details(), candidates, "192.0.2.42").endpoint, "192.0.2.42:28000");
  assert.equal(resolve(details(), candidates, "198.51.100.99").endpoint, "203.0.113.8:28000");
  assert.equal(resolve(details(), candidates.filter((item) => item.kind !== "public")).endpoint, "198.51.100.4:28000");
  assert.equal(resolve(details({ summary: { id: "bound", module_id: "valheim", bind_ip: "192.0.2.55" } }),
    candidates, "203.0.113.8").endpoint, "192.0.2.55:28000");
});

test("Valheim crossplay never shares an adapter address, including a saved direct-connect preference", () => {
  for (const bind_ip of ["0.0.0.0", "198.51.100.4"]) {
    const value = details({ summary: { id: "crossplay", name: "Crossplay room", module_id: "valheim", bind_ip },
      settings_json: '{"crossplay_enabled":true}' });
    assert.deepEqual(buildShareEndpoints(value, candidates, "en-US", t), []);
    assert.equal(resolve(value, candidates, "198.51.100.4"), null);
    assert.equal(buildJoinEndpoint(value, candidates, "en-US", t), "");
    const invitation = buildInviteText(value, [], "en-US", t, candidates);
    assert.match(invitation, /current startup log/);
    assert.match(invitation, /crossplay server list/);
    assert.doesNotMatch(invitation, /198\.51\.100\.4|203\.0\.113\.8|:28000/);
  }
});

test("Valheim direct mode keeps adapter addresses and crossplay guidance is localized", () => {
  const direct = details({ settings_json: '{"crossplay_enabled":false}' });
  assert.deepEqual(buildShareEndpoints(direct, candidates, "zh-CN", t), buildShareEndpoints(details(), candidates, "zh-CN", t));
  assert.equal(resolve(direct, candidates, "198.51.100.4").endpoint, "198.51.100.4:28000");
  assert.equal(valheimCrossplayJoinGuidance(direct, t), null);
  const catalogs = [require("../src/i18n/games/valheim.zh-cn.ts").ZH_CN_VALHEIM_MESSAGES,
    require("../src/i18n/games/valheim.en.ts").EN_US_VALHEIM_MESSAGES];
  for (const [index, catalog] of catalogs.entries()) {
    const guidance = valheimCrossplayJoinGuidance(details({ settings_json: '{"crossplay_enabled":true}' }),
      (key, _params, fallback) => catalog[key] ?? fallback ?? key);
    assert.ok(guidance);
    assert.match(guidance.summary, index === 0 ? /跨平台.*加入码/ : /Crossplay: join code/);
    assert.match(guidance.instructions, index === 0 ? /本次启动日志.*跨平台服务器列表/ : /current startup log.*crossplay server list/);
    assert.match(guidance.instructions, /Radmin/);
  }
});

test("a wildcard or proxy-only fallback never becomes a copyable join endpoint", () => {
  for (const bind_ip of ["0.0.0.0", "::", "[::]", ""]) {
    const value = details({ summary: { id: "wildcard", module_id: "valheim", bind_ip } });
    assert.equal(resolve(value, []), null);
    assert.equal(resolve(value, candidates.filter((item) => ["all", "proxy"].includes(item.kind))), null);
  }
});

test("missing and invalid saved ports are unavailable rather than fabricated", () => {
  assert.equal(resolve(details({ ports: [] })), null);
  for (const port of [0, -1, 65536, 2456.5, NaN]) {
    assert.equal(resolve(details({ ports: [{ name: "game", protocol: "udp", port }] })), null);
  }
});

test("Core Keeper relay shares its effective Game ID and direct mode uses the saved game port", () => {
  const relay = details({ summary: { id: "core-fixture", module_id: "corekeeper", bind_ip: "0.0.0.0" },
    ports: [], settings_json: JSON.stringify({ direct_connection_enabled: false, game_id: "fixturegameid12345" }) });
  assert.deepEqual(resolve(relay, []), { address: "fixturegameid12345", endpoint: "fixturegameid12345", kind: "relay", label: "Steam relay Game ID" });
  assert.equal(resolve({ ...relay, settings_json: "{}" }, []).endpoint, "lgmcorefixturecorekeeper");
  assert.equal(resolve({ ...relay, ports: [{ name: "game", protocol: "udp", port: 27031 }],
    settings_json: '{"direct_connection_enabled":true}' }).endpoint, "203.0.113.8:27031");
});

test("card preference reads the existing per-instance setting and tolerates unavailable storage", () => {
  const previousWindow = global.window;
  try {
    global.window = { localStorage: { getItem(key) {
      assert.equal(key, "langame.join-address.fixture-server");
      return "192.0.2.42";
    } } };
    assert.equal(readPreferredJoinAddress("fixture-server"), "192.0.2.42");
    global.window = { get localStorage() { throw new Error("Storage denied"); } };
    assert.equal(readPreferredJoinAddress("fixture-server"), null);
  } finally {
    if (previousWindow === undefined) delete global.window;
    else global.window = previousWindow;
  }
});

test("IPv6 join addresses use brackets while retaining the saved port", () => {
  for (const bind_ip of ["2001:db8::1", "[2001:db8::1]"]) {
    assert.equal(resolve(details({ summary: { id: "ipv6", module_id: "valheim", bind_ip } }), []).endpoint,
      "[2001:db8::1]:28000");
  }
});


const astroneerDetails = (settings, ports = [{ name: "game", protocol: "udp", port: 8777 }]) => details({
  summary: { id: "astroneer-fixture", name: "Astroneer room", module_id: "astroneer", bind_ip: "0.0.0.0" },
  settings_json: typeof settings === "string" ? settings : JSON.stringify(settings), ports
});
const astroneerCandidates = [
  { address: "192.168.1.40", kind: "lan" },
  { address: "8.8.4.4", kind: "public" },
  { address: "26.10.20.30", kind: "overlay", family_name: "Radmin VPN", adapter_name: "Radmin VPN" }
];

test("ASTRONEER shares only the advertised Radmin address across card, copy and invitation", () => {
  const value = astroneerDetails({ connection_mode: "radmin", public_ip: "26.10.20.30" }, [
    { name: "console", protocol: "tcp", port: 1234 }, { name: "game", protocol: "udp", port: 18777 }
  ]);
  const endpoints = buildShareEndpoints(value, astroneerCandidates, "en-US", t);
  assert.deepEqual(endpoints, [{ address: "26.10.20.30", endpoint: "26.10.20.30:18777", kind: "overlay", label: "Radmin VPN" }]);
  for (const preference of [null, "192.168.1.40", "8.8.4.4"]) {
    assert.equal(resolve(value, astroneerCandidates, preference).endpoint, "26.10.20.30:18777");
  }
  assert.equal(buildJoinEndpoint(value, astroneerCandidates, "en-US", t), "26.10.20.30:18777");
  const invitation = buildInviteText(value, [], "en-US", t, astroneerCandidates);
  assert.match(invitation, /26\.10\.20\.30:18777/);
  assert.match(invitation, /same Radmin VPN network/);
  assert.doesNotMatch(invitation, /192\.168\.1\.40|8\.8\.4\.4|:1234/);
  assert.match(buildInviteText(value, [], "en-US", t, []), /same Radmin VPN network/);
  assert.equal(buildShareEndpoints(value, [], "en-US", t)[0].label, "Radmin VPN");
});

test("ASTRONEER preserves an explicit legacy or forwarded endpoint absent from local adapters", () => {
  for (const settings of [{ public_ip: "8.8.4.4" }, { connection_mode: "manual", public_ip: "8.8.4.4" }]) {
    const value = astroneerDetails(settings);
    assert.equal(resolve(value, [], "26.10.20.30").endpoint, "8.8.4.4:8777");
    assert.equal(buildShareEndpoints(value, [], "en-US", t)[0].label, "Advertised address");
  }
  const updated = astroneerDetails({ connection_mode: "public", public_ip: "8.8.4.4" });
  assert.equal(resolve(updated, astroneerCandidates, "26.10.20.30").endpoint, "8.8.4.4:8777");
});

test("ASTRONEER has no adapter fallback when its advertised address is missing or invalid", () => {
  for (const settings of ["{}", "[]", "invalid", ...["", "0.0.0.0", "127.0.0.1", "26.10.20.30:8777", "260.1.2.3"]
    .map((public_ip) => JSON.stringify({ public_ip }))]) {
    const value = astroneerDetails(settings);
    assert.deepEqual(buildShareEndpoints(value, astroneerCandidates, "en-US", t), []);
    assert.equal(resolve(value, astroneerCandidates, "26.10.20.30"), null);
    assert.equal(buildJoinEndpoint(value, astroneerCandidates, "en-US", t), "");
    const invitation = buildInviteText(value, [], "en-US", t, astroneerCandidates);
    assert.match(invitation, /Select the player join address/);
    assert.doesNotMatch(invitation, /26\.10\.20\.30|8\.8\.4\.4|192\.168\.1\.40|:8777/);
  }
});

test("ASTRONEER never substitutes its management port or an invalid game port", () => {
  const invalidPorts = [[], [{ name: "console", protocol: "tcp", port: 1234 }],
    [{ name: "game", protocol: "tcp", port: 8777 }],
    ...[0, -1, 65536, 8777.5].map((port) => [{ name: "game", protocol: "udp", port }])];
  for (const ports of invalidPorts) {
    const value = astroneerDetails({ connection_mode: "radmin", public_ip: "26.10.20.30" }, ports);
    assert.deepEqual(buildShareEndpoints(value, astroneerCandidates, "en-US", t), []);
    assert.equal(resolve(value, astroneerCandidates), null);
    assert.equal(buildJoinEndpoint(value, astroneerCandidates, "en-US", t), "");
  }
});


test("ASTRONEER shares a persisted LAN or VPN endpoint without replacing it with another adapter", () => {
  for (const address of ["192.168.1.40", "10.20.30.40", "100.64.1.2"]) {
    const value = astroneerDetails({ connection_mode: "manual", public_ip: address });
    assert.equal(buildShareEndpoints(value, astroneerCandidates, "en-US", t)[0].endpoint, `${address}:8777`);
    assert.equal(resolve(value, astroneerCandidates, "26.10.20.30").endpoint, `${address}:8777`);
    assert.match(buildInviteText(value, [], "en-US", t, astroneerCandidates), new RegExp(address.replaceAll(".", "\\.")));
  }
});
