const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (loaded, filename) => {
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const filename = path.join(__dirname, "../src/views/servers/RuntimeSurfaceWorkbench.tsx");
const originalRequire = Module.createRequire(filename);
const loaded = new Module(filename, module);
loaded.filename = filename;
loaded.require = (id) => {
  if (id === "../../i18n") return {
    selectLocaleText: originalRequire("../../i18n-config.ts").selectLocaleText,
    useI18n: () => ({ locale: "en", t: (key, _params, fallback) => fallback || key })
  };
  if (id === "@tauri-apps/api/core") return { isTauri: () => false };
  if (id === "@tauri-apps/api/event") return { listen: async () => () => {} };
  if (id === "../../api") return { readInstanceLogDocument: async () => { throw new Error("Unexpected log request in static render"); } };
  if (id === "../../app-state") return { describeError: String };
  if (id === "../../components/ShellIcon") return { ShellIcon: () => null };
  return originalRequire(id);
};
loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);

function healthLight(status, healthStatus, startupPending = false) {
  const html = renderToStaticMarkup(React.createElement(loaded.exports.RuntimeSurfaceWorkbench, {
    details: { summary: { id: "fixture", module_id: "dontstarve", status, active_process_count: status === "running" ? 1 : 0 }, active_run: null },
    runtime: { recent_runs: [], log_tail: { lines: [], total_lines: 0, truncated: false, read_error: null }, health: { status: healthStatus, summary: `Native health: ${healthStatus}`, reason: null }, diagnostics: [] },
    runtimeWindows: null, startupPending, launchHostSurface: "managed_terminal",
    onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {}
  }));
  return html.match(/<span class="server-runtime-health-light[^>]*>/)?.[0] ?? "";
}

test("native health errors and failed instances cannot render an all-clear lamp", () => {
  for (const [instance, health] of [["running", "error"], ["error", "ready"]]) {
    const light = healthLight(instance, health);
    assert.match(light, /is-danger/);
    assert.doesNotMatch(light, /All clear/);
  }
});

test("native warning is visible even without structured diagnostics", () => {
  const light = healthLight("running", "warning");
  assert.match(light, /is-warning/);
  assert.match(light, /Native health: warning/);
});

test("pending startup and stopped instances are not reported healthy", () => {
  for (const light of [healthLight("stopped", "stopped"), healthLight("stopped", "stopped", true)]) {
    assert.doesNotMatch(light, /is-ready|All clear/);
  }
  assert.match(healthLight("running", "ready"), /is-ready/);
});
