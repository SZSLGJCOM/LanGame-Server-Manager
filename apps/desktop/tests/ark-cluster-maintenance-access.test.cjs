const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load({ desktop, development, hostname, token = null }) {
  let requests = 0;
  const window = { location: { hostname, protocol: "http:", hash: "", pathname: "/", search: "" },
    sessionStorage: { getItem: () => token }, history: { replaceState: () => {} } };
  const core = { isTauri: () => desktop, invoke: () => { requests++; throw new Error("Unexpected native request"); } };
  const transportPath = path.resolve(__dirname, "../src/api-transport.ts");
  const transport = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(transportPath, "utf8").replaceAll("import.meta.env.DEV", String(development)), transportPath),
    { exports: transport.exports, require: () => core, window, URLSearchParams });
  const filename = path.resolve(__dirname, "../src/views/servers/ArkClusterMaintenance.tsx");
  const component = { exports: {} };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports: component.exports,
    require(id) {
      if (id === "react") return React;
      if (id === "react/jsx-runtime") return require(id);
      if (id === "@tauri-apps/api/core") return core;
      if (id === "../../api-transport") return transport.exports;
      if (id === "../../i18n") return { useI18n: () => ({ locale: "en-US" }), selectLocaleText: (_locale, _zh, en) => en };
      if (id === "../../api") return new Proxy({}, { get() { return () => { requests++; throw new Error("Unexpected backup API request"); }; } });
      if (id === "../../app-state") return { describeError: String };
      if (id === "../../components/ActivityNotice") return { ActivityNotice: () => null };
      if (id === "./ArkClusterBackupPanel") return { ArkClusterBackupPanel: () => null };
      throw new Error(`Unexpected dependency: ${id}`);
    }
  });
  return { render: () => component.exports.ArkClusterMaintenance({ instanceId: "a", report: null, busy: false,
    onChanged: async () => {}, onBusyChange: () => {} }), get requests() { return requests; } };
}

for (const hostname of ["192.168.1.8", "127.0.0.1"]) {
  test(`production LAN at ${hostname} displays only the local-desktop explanation`, () => {
    const state = load({ desktop: false, development: false, hostname });
    const view = state.render();
    assert.equal(view.type, "p");
    assert.match(renderToStaticMarkup(view), /require the local LanGame desktop app/);
    assert.doesNotMatch(renderToStaticMarkup(view), /<button|<input/);
    assert.equal(state.requests, 0);
  });
}
test("desktop always uses local maintenance and a token-free development loopback can preview it", () => {
  for (const settings of [
    { desktop: true, development: false, hostname: "192.168.1.8", token: "fixture" },
    { desktop: false, development: true, hostname: "127.0.0.1" },
    { desktop: false, development: true, hostname: "localhost" }
  ]) assert.equal(typeof load(settings).render().type, "function");
});
test("development LAN and authenticated loopback cannot mount local-only operations", () => {
  for (const settings of [
    { desktop: false, development: true, hostname: "192.168.1.8" },
    { desktop: false, development: true, hostname: "127.0.0.1", token: "fixture" }
  ]) assert.equal(load(settings).render().type, "p");
});
