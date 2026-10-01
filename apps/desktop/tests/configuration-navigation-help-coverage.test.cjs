const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("module.exports = {};", module.filename);
};

const desktopRoot = path.resolve(__dirname, "..");
const settingsRoot = path.join(desktopRoot, "src", "views", "settings");
const { parseGuidedSettingsSchema } = require(path.join(settingsRoot, "guided-settings.ts"));
const { listSettingsModuleIds } = require(path.join(settingsRoot, "module-registry.ts"));
const {
  buildConfigurationWorkspaceModel,
  configurationNavigationRoots
} = require(path.join(settingsRoot, "configuration-workspace-model.ts"));
const { ConfigurationSectionNavigation } = require(path.join(settingsRoot, "ConfigurationSectionNavigation.tsx"));
const { EN_US_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages.ts"));
const { ZH_CN_MESSAGES } = require(path.join(desktopRoot, "src", "i18n-messages-zh-cn.ts"));

function flattenNodes(nodes) {
  return nodes.flatMap((node) => [node, ...flattenNodes(node.children)]);
}

function escapeHtml(text) {
  return text.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#x27;"
  })[character]);
}

const modules = listSettingsModuleIds().map((moduleId) => ({
  summary: { id: moduleId, name: moduleId },
  schema_json: fs.readFileSync(path.join(desktopRoot, "..", "..", "modules", moduleId, "schema.json"), "utf8")
}));

for (const [locale, catalog] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]]) {
  const translate = (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
    /\{\s*([\w.]+)\s*\}/g,
    (match, paramKey) => params?.[paramKey] == null ? match : String(params[paramKey])
  );
  const navigation = modules.map((moduleDetails) => {
    const schema = parseGuidedSettingsSchema(moduleDetails, locale, translate);
    assert.equal(schema.parseError, null, `${locale}: ${moduleDetails.summary.id} schema`);
    return {
      moduleId: moduleDetails.summary.id,
      nodes: flattenNodes(configurationNavigationRoots(buildConfigurationWorkspaceModel(schema).roots))
    };
  });

  test(`${locale}: every visible module category has meaningful hover help`, () => {
    assert.ok(navigation.length > 0, "The module registry must supply configuration pages");
    for (const { moduleId, nodes } of navigation) {
      assert.ok(nodes.length > 0, `${moduleId} must expose its configuration navigation`);
    }
    const missing = navigation.flatMap(({ moduleId, nodes }) => nodes
      .filter((node) => !node.description?.trim() || node.description.trim() === node.title.trim())
      .map((node) => `${moduleId}.${node.id}`));
    assert.deepEqual(missing, [], "Configuration categories need descriptions beyond their labels");
  });

  test(`${locale}: rendered category controls reference their complete help text`, () => {
    for (const { moduleId, nodes } of navigation) {
      for (const node of nodes) {
        const context = `${locale}: ${moduleId}.${node.id}`;
        assert.ok(node.description?.trim(), `${context} is missing its help text`);
        const html = renderToStaticMarkup(React.createElement(ConfigurationSectionNavigation, {
          roots: [node],
          selectedSectionId: node.id,
          ariaLabel: "Configuration categories",
          onSelectSection: () => {}
        }));
        const descriptionId = html.match(/aria-describedby="([^"]+)"/)?.[1];
        assert.ok(descriptionId, `${context} must expose accessible help`);
        assert.ok(html.includes(
          `<span id="${descriptionId}" class="configuration-field-help-a11y" role="tooltip">${escapeHtml(node.description.trim())}</span>`
        ), `${context} must link to its complete description`);
        assert.doesNotMatch(html, / title="/, `${context} must use the shared help instead of native title`);
      }
    }
  });
}
