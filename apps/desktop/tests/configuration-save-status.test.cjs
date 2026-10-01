const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const { ConfigurationSaveStatus } = require("../src/views/settings/ConfigurationSaveStatus.tsx");
const { ZH_CN_SETTINGS_MESSAGES } = require("../src/i18n-messages-zh-settings.ts");
const t = (key, _params, fallback) => ZH_CN_SETTINGS_MESSAGES[key] ?? fallback;

function render(status, validationBlocked = false) {
  return renderToStaticMarkup(React.createElement(ConfigurationSaveStatus, { status, validationBlocked, t }));
}

test("pending, saving and acknowledged settings have distinct accessible feedback", () => {
  for (const [state, label] of [["dirty", "更改等待保存"], ["saving", "正在保存配置"], ["saved", "配置已保存"]]) {
    const html = render({ state });
    assert.match(html, new RegExp(label));
    assert.match(html, /role="status"/);
    assert.match(html, new RegExp(`class="shell-activity-notice is-${state === "saved" ? "success" : "info"}"`));
  }
});

test("validation, failed writes and conflicts never report a saved configuration", () => {
  assert.match(render({ state: "saved" }, true), /请先修正无效设置/);
  for (const state of ["failed", "conflict"]) {
    const html = render({ state, message: "Write rejected" });
    assert.match(html, /更改尚未保存/);
    assert.doesNotMatch(html, /配置已保存/);
  }
});
