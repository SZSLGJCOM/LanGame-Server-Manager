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
