const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const modules = new Map();
function loadSource(filename) {
  if (filename.endsWith("?raw")) return fs.readFileSync(filename.slice(0, -4), "utf8");
  if (filename.endsWith(".json")) return JSON.parse(fs.readFileSync(filename, "utf8"));
  if (!path.extname(filename)) filename += ".ts";
  if (modules.has(filename)) return modules.get(filename);
  const exports = {};
  modules.set(filename, exports);
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: request => loadSource(path.resolve(path.dirname(filename), request))
  }, { filename });
  return exports;
}

const { resolveRuntimeConsoleTransport: resolve } = loadSource(path.resolve(__dirname, "../src/runtime-console-transport.ts"));
const manifest = loadSource(path.resolve(__dirname, "../src/api-mock/module-manifest.ts"));
function actualModule(id) {
  return { summary: { id }, runtime: {
    player_actions: manifest.parseMockPlayerActionsFromModuleToml(id),
    shutdown: manifest.parseMockRuntimeShutdownFromModuleToml(id)
  } };
}
function moduleWith(actions, shutdown = []) {
  return { summary: { id: "fixture" }, runtime: { player_actions: actions, shutdown: { commands: shutdown } } };
}
function plain(value) { return JSON.parse(JSON.stringify(value)); }
const remoteSettings = JSON.stringify({
  rcon_password: "fixture-password", admin_password: "fixture-admin-password", telnet_password: "console-pass",
  rcon_enabled: true, enable_rcon: true, rcon_web: true, telnet_enabled: true, rest_api_enabled: true
});
const rcon = { transport: "source_rcon", port_name: "rcon", password_setting_key: "rcon_password", enabled_setting_key: "rcon_enabled" };

test("Windrose native GUI console requires no RCON credentials or valid settings JSON", () => {
  for (const settings of ["{}", "not-json", '{"rcon_enabled":false}']) {
    const result = resolve(actualModule("windrose"), settings);
    assert.equal(result.available, true);
    assert.deepEqual(plain(result.options), { transport: "unreal_console" });
    assert.equal(result.hint, null);
  }
});

test("all current modules select their declared text channel without sending action-only protocols", () => {
  const expected = {
    abioticfactor: "stdin", arksurvivalascended: "source_rcon", arksurvivalevolved: "source_rcon",
    astroneer: "stdin", barotrauma: "stdin", conanexiles: "source_rcon", corekeeper: "stdin",
    dontstarve: "stdin", enshrouded: "stdin", humanitz: "source_rcon", minecraft: "source_rcon",
    necesse: "stdin", nightingale: "stdin", palworld: "stdin", projectzomboid: "source_rcon",
    returntomoria: "stdin", rimworld: "stdin", romestead: "stdin", runescapedragonwilds: "stdin",
    rust: "websocket_rcon", satisfactory: "stdin", scum: "stdin", sevendaystodie: "telnet",
    sonsoftheforest: "stdin", soulmask: "source_rcon", squad: "source_rcon", terraria: "stdin",
    theforest: "stdin", unturned: "stdin", valheim: "stdin", vrising: "source_rcon", windrose: "unreal_console"
  };
  assert.equal(Object.keys(expected).length, 32);
  for (const [id, transport] of Object.entries(expected)) {
    const result = resolve(actualModule(id), remoteSettings);
    assert.equal(result.available, true, id);
    assert.equal(result.options.transport, transport, id);
    assert.equal(JSON.stringify(result).includes("fixture-password"), false, id);
  }
});

test("Minecraft keeps its declared native input when RCON is disabled and restores RCON only when enabled", () => {
  const module = actualModule("minecraft");
  const result = resolve(module, JSON.stringify({ enable_rcon: false, rcon_password: "fixture-password" }));
  assert.equal(result.available, true);
  assert.deepEqual(plain(result.options), { transport: "stdin" });
  assert.match(result.hint.en, /not enabled.*enable_rcon.*declared native input/);
  assert.equal(resolve(module, remoteSettings).options.transport, "source_rcon");
});

test("native consoles keep working without remote credentials or parseable settings", () => {
  for (const id of ["dontstarve", "barotrauma", "necesse", "terraria", "unturned"]) {
    const result = resolve(actualModule(id), "not JSON");
    assert.equal(result.available, true, id);
    assert.deepEqual(plain(result.options), { transport: "stdin" }, id);
    assert.equal(result.hint, null, id);
  }
});

test("declared stdin fallback remains usable when remote credentials are absent", () => {
  const result = resolve(actualModule("projectzomboid"), "{}");
  assert.equal(result.available, true);
  assert.equal(result.options.transport, "stdin");
  assert.match(result.hint.en, /no password.*rcon_password.*declared native input/);
});

test("remote routes without a native fallback explain disabled and missing-password configuration", () => {
  const module = actualModule("conanexiles");
  const disabled = resolve(module, JSON.stringify({ rcon_enabled: false }));
  assert.equal(disabled.available, false);
  assert.equal(disabled.options, undefined);
  assert.match(disabled.hint.en, /not enabled.*rcon_enabled/);
  const missingPassword = resolve(module, JSON.stringify({ rcon_enabled: true }));
  assert.equal(missingPassword.available, false);
  assert.match(missingPassword.hint.en, /no password.*rcon_password/);
});

test("remote settings must be an object and do not turn an unavailable route into native input", () => {
  for (const settings of ["{", "[]", "null", "1", '"string"']) {
    const result = resolve(actualModule("conanexiles"), settings);
    assert.equal(result.available, false);
    assert.match(result.hint.en, /valid JSON object/);
  }
});

test("boolean settings follow native dispatch semantics and never mutate configuration", () => {
  const module = moduleWith([rcon]);
  const settings = { rcon_enabled: " On ", rcon_password: "fixture-password" };
  const original = JSON.stringify({ module, settings });
  assert.equal(resolve(module, JSON.stringify(settings)).available, true);
  for (const disabled of [false, "off", "0", 1, null]) {
    assert.equal(resolve(module, JSON.stringify({ ...settings, rcon_enabled: disabled })).available, false);
  }
  assert.equal(JSON.stringify({ module, settings }), original);
});

test("Telnet keeps its supported passwordless and missing-enable-flag behavior", () => {
  const result = resolve(actualModule("sevendaystodie"), "{}");
  assert.equal(result.available, true);
  assert.equal(result.options.transport, "telnet");
  assert.equal(result.options.passwordSettingKey, "telnet_password");
  const disabled = resolve(actualModule("sevendaystodie"), '{"telnet_enabled":false}');
  assert.equal(disabled.options.transport, "stdin");
});

test("duplicate actions and shutdown declarations share one normalized endpoint", () => {
  const module = moduleWith([rcon, { ...rcon, port_name: " RCON ", transport: " SOURCE_RCON " }], [rcon]);
  const result = resolve(module, remoteSettings);
  assert.equal(result.available, true);
  assert.deepEqual(plain(result.options), {
    transport: "source_rcon", portName: "rcon", passwordSettingKey: "rcon_password", enabledSettingKey: "rcon_enabled"
  });
});

test("different endpoints, credentials or protocols are ambiguous even across action and shutdown declarations", () => {
  for (const alternative of [
    { ...rcon, port_name: "other" },
    { ...rcon, password_setting_key: "admin_password" },
    { ...rcon, transport: "websocket_rcon" },
    { transport: "stdin" }
  ]) {
    const result = resolve(moduleWith([rcon], [alternative]), remoteSettings);
    assert.equal(result.available, false);
    assert.equal(result.options, undefined);
    assert.match(result.hint.en, /ambiguous/);
  }
});

test("a unique enabled route can be selected without enabling another route", () => {
  const module = moduleWith([rcon, { ...rcon, port_name: "other", enabled_setting_key: "other_enabled" }]);
  const result = resolve(module, remoteSettings);
  assert.equal(result.available, true);
  assert.equal(result.options.portName, "rcon");
});

test("specialized actions and Ctrl+C do not assert a verified interactive console", () => {
  for (const module of [moduleWith([]), actualModule("abioticfactor"), actualModule("palworld"), moduleWith([{ transport: "humanitz_rcon" }])]) {
    const result = resolve(module, remoteSettings);
    assert.equal(result.available, true);
    assert.deepEqual(plain(result.options), { transport: "stdin" });
    assert.match(result.hint.en, /receipt and execution require a server response/);
  }
});

test("module information must load before selecting a route", () => {
  const result = resolve(null, remoteSettings);
  assert.equal(result.available, false);
  assert.equal(result.options, undefined);
  assert.match(result.hint.en, /Module information has not loaded/);
  assert.equal(resolve(moduleWith([]), remoteSettings).available, true);
});

test("ARK map commands require configured RCON and do not inherit the shutdown stdin fallback", () => {
  for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
    const module = actualModule(id);
    const disabled = resolve(module, '{"rcon_enabled":false,"admin_password":"fixture-admin-password"}');
    assert.equal(disabled.available, false, id);
    assert.equal(disabled.options, undefined, id);
    assert.match(disabled.hint.en, /not enabled.*rcon_enabled/, id);
    const missingPassword = resolve(module, '{"rcon_enabled":true}');
    assert.equal(missingPassword.available, false, id);
    assert.match(missingPassword.hint.en, /no password.*admin_password/, id);
    assert.equal(resolve(module, remoteSettings).options.transport, "source_rcon", id);
  }
});

test("omitted transport is a declared stdin action and shutdown-only routes retain dispatch metadata", () => {
  assert.equal(resolve(moduleWith([{}]), "{}").hint, null);
  const result = resolve(moduleWith([], [{ ...rcon, password_setting_key: "admin_password" }]), remoteSettings);
  assert.equal(result.available, true);
  assert.equal(result.options.passwordSettingKey, "admin_password");
});
