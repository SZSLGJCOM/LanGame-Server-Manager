const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
function modelFor(id) {
  const schema = parseGuidedSettingsSchema({
    summary: { id, name: id },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules", id, "schema.json"), "utf8")
  });
  assert.equal(schema.parseError, null);
  return buildConfigurationWorkspaceModel(schema);
}
function fieldSections(model) {
  return Object.fromEntries(model.items.map((item) => [item.fieldKey, item.sectionId]));
}

test("7 Days to Die separates room identity, native access policy and remote services", () => {
  const model = modelFor("sevendaystodie");
  const sections = fieldSections(model);
  assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey).sort(),
    ["server_name", "server_description", "server_website_url", "server_login_confirmation_text", "server_password", "max_players", "visibility", "region", "language", "game_world", "world_name"].sort());
  assert.equal(sections.command_permissions, "access");
  assert.equal(model.items.find((item) => item.fieldKey === "command_permissions").owner, "configuration");
  for (const key of ["telnet_enabled", "telnet_password", "server_allow_crossplay", "web_dashboard_enabled"]) assert.equal(sections[key], "network", key);
  for (const key of ["ignore_eos_sanctions", "eac_enabled", "twitch_server_permission"]) assert.equal(sections[key], "access", key);
  assert.equal(sections.terminal_window_enabled, "runtime");
  assert.equal(sections.game_mode, "world");
});

test("7 Days to Die command permissions have one descriptive heading and no roster migration notice", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const { ZH_CN_SEVEN_DAYS_TO_DIE_MESSAGES } = require("../src/i18n/games/sevendaystodie.zh-cn.ts");
  const { SevenDaysServerAdminPanel } = require("../src/views/settings/SevenDaysServerAdminPanel.tsx");
  const t = (key, _params, fallback) => ZH_CN_SEVEN_DAYS_TO_DIE_MESSAGES[key] ?? fallback ?? key;
  const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
    value: { locale: "zh-CN", setLocale() {}, t }
  }, React.createElement(SevenDaysServerAdminPanel, {
    sectionId: "access", settings: { command_permissions: [] }, disabled: false, onPatch() {}
  })));
  assert.equal((html.match(/<h4\b/gu) ?? []).length, 1);
  assert.match(html, />控制台命令权限<\/h4>/u);
  assert.doesNotMatch(html, /<div class="detail-label">命令<\/div>|玩家名单|玩家访问/u);
});
