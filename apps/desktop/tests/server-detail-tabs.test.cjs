const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const { ServerDetailTabs, resolveDetailTabKey } = require("../src/views/servers/ServerDetailTabs.tsx");
const tabs = [
  { id: "runtime", label: "Runtime", icon: "terminal" },
  { id: "settings", label: "Configuration", icon: "settings" },
  { id: "mods", label: "Mods", icon: "package", disabled: true },
  { id: "players", label: "Players", icon: "users" }
];

test("arrow keys skip disabled tabs and wrap through available workflows", () => {
  assert.equal(resolveDetailTabKey(tabs, "settings", "ArrowRight").id, "players");
  assert.equal(resolveDetailTabKey(tabs, "players", "ArrowLeft").id, "settings");
  assert.equal(resolveDetailTabKey(tabs, "players", "ArrowRight").id, "runtime");
  assert.equal(resolveDetailTabKey(tabs, "runtime", "ArrowLeft").id, "players");
});

test("Home and End navigate to enabled endpoints without consuming other keys", () => {
  assert.equal(resolveDetailTabKey(tabs, "settings", "Home").id, "runtime");
  assert.equal(resolveDetailTabKey(tabs, "settings", "End").id, "players");
  for (const key of ["Tab", "Enter", "Escape", "ArrowDown"]) {
    assert.equal(resolveDetailTabKey(tabs, "settings", key), null);
  }
  assert.equal(resolveDetailTabKey([], "settings", "Home"), null);
  assert.equal(resolveDetailTabKey(tabs.map((tab) => ({ ...tab, disabled: true })), "settings", "End"), null);
});

test("only the selected tab enters the Tab sequence and all tabs identify their panel", () => {
  const html = renderToStaticMarkup(React.createElement(ServerDetailTabs, {
    tabs, activeTab: "settings", panelId: "instance-details", label: "Instance workflows", onSelect() {}
  }));
  assert.equal((html.match(/tabindex="0"/g) ?? []).length, 1);
  assert.equal((html.match(/aria-selected="true"/g) ?? []).length, 1);
  assert.equal((html.match(/aria-controls="instance-details"/g) ?? []).length, tabs.length);
  assert.match(html, /id="instance-details-settings"[^>]*aria-selected="true"[^>]*tabindex="0"/);
  assert.match(html, /id="instance-details-mods"[^>]*aria-disabled="true"[^>]*disabled=""/);
  const viewSource = fs.readFileSync(path.resolve(__dirname, "../src/views/ServersView.tsx"), "utf8");
  assert.match(viewSource, /id=\{detailPanelId\} role="tabpanel" aria-labelledby=\{`\$\{detailPanelId\}-\$\{activeDetailTab\}`\}/);
});

test("unavailable tools expose an accessible explanation and are skipped by tab navigation", () => {
  const unavailable = { id: "gm", label: "Tools", icon: "zap", disabled: true,
    disabledReason: "This game does not currently provide server tools." };
  const supportedTabs = [...tabs, unavailable];
  const html = renderToStaticMarkup(React.createElement(ServerDetailTabs, {
    tabs: supportedTabs, activeTab: "runtime", panelId: "instance-details", label: "Instance workflows", onSelect() {}
  }));
  assert.match(html, /class="server-detail-tab-help"[^>]*tabindex="0"[^>]*aria-label="Tools"[^>]*aria-describedby="instance-details-gm-help"/);
  assert.match(html, /id="instance-details-gm"[^>]*aria-disabled="true"[^>]*disabled=""/);
  assert.match(html, /id="instance-details-gm-help"[^>]*role="tooltip">This game does not currently provide server tools\.<\/span>/);
  assert.equal(resolveDetailTabKey(supportedTabs, "players", "ArrowRight").id, "runtime");
  assert.equal(resolveDetailTabKey(supportedTabs, "runtime", "End").id, "players");
});
