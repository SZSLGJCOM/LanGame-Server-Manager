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

test("reloaded language hooks still read the mounted provider's context", () => {
  const providerPath = path.resolve(__dirname, "../src/i18n.tsx");
  const { I18nContext } = require("../src/i18n-context.ts");
  const initial = require(providerPath);
  const value = { locale: "zh-CN", setLocale() {}, t: () => "配置已保存" };
  function render(useI18n) {
    function Consumer() {
      const context = useI18n();
      assert.equal(context, value);
      return React.createElement("span", { lang: context.locale }, context.t("saved"));
    }
    return renderToStaticMarkup(React.createElement(I18nContext.Provider, { value }, React.createElement(Consumer)));
  }
  assert.match(render(initial.useI18n), /配置已保存/);
  delete require.cache[providerPath];
  const reloaded = require(providerPath);
  assert.notEqual(reloaded.useI18n, initial.useI18n);
  assert.equal(render(reloaded.useI18n), render(initial.useI18n));
  assert.throws(() => renderToStaticMarkup(React.createElement(() => {
    reloaded.useI18n();
    return null;
  })), /useI18n must be used inside I18nProvider/);
});
