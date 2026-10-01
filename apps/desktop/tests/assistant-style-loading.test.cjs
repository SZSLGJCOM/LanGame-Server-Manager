const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const postcss = require("postcss");
const { parseSource, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const stylesheetUrl = "/src/app.css";

function compiledStyles(result) {
  assert.ok(result?.code, "Vite must return a transformed stylesheet module");
  let css;
  visitSyntax(parseSource(result.code, "app.css.js"), (node) => {
    if (node.type === "VariableDeclarator" && node.id?.value === "__vite__css") {
      assert.equal(node.init?.type, "StringLiteral", "Vite must emit CSS for the browser");
      css = node.init.value;
    }
  });
  assert.equal(typeof css, "string", "the transformed module must contain its CSS payload");
  return postcss.parse(css);
}

function assertAssistantStyles(result, phase) {
  const rules = new Map();
  compiledStyles(result).walkRules((rule) => {
    for (const selector of rule.selectors) {
      const declarations = rules.get(selector) ?? new Map();
      rule.walkDecls((declaration) => declarations.set(declaration.prop, declaration.value));
      rules.set(selector, declarations);
    }
  });
  function declaration(selector, property) {
    const value = rules.get(selector)?.get(property);
    assert.ok(value, `${phase}: ${selector} must deliver ${property} to the browser`);
    return value;
  }
  for (const selector of [".assistant-island-logo", ".assistant-panel-controls svg"]) {
    for (const property of ["width", "height"]) {
      const value = declaration(selector, property);
      assert.match(value, /^\d+(?:\.\d+)?px$/, `${phase}: ${selector} needs explicit ${property}`);
      assert.ok(parseFloat(value) > 0 && parseFloat(value) <= 64,
        `${phase}: ${selector} must stay at icon scale`);
    }
  }
  assert.equal(declaration(".assistant-chat-feed", "overflow-y"), "auto");
  assert.equal(declaration(".assistant-history-drawer", "position"), "absolute");
  assert.equal(declaration(".assistant-panel-head", "display"), "flex");
  assert.equal(declaration(".assistant-panel-controls", "flex"), "0 0 auto");
}

test("Vite delivers all LAN styles on first load and after module invalidation", async () => {
  const { createServer } = await import("vite");
  const temporaryRoot = fs.realpathSync(os.tmpdir());
  const temporaryDirectory = fs.mkdtempSync(path.join(temporaryRoot, "langame-assistant-css-"));
  let server;
  try {
    server = await createServer({
      root: desktopRoot,
      configFile: path.join(desktopRoot, "vite.config.ts"),
      // The runner loads the real project config without writing a bundled config beside it.
      configLoader: "runner",
      cacheDir: path.join(temporaryDirectory, "vite-cache"),
      logLevel: "error",
      optimizeDeps: { noDiscovery: true, include: [] },
      server: { middlewareMode: true, watch: null, ws: false }
    });

    const first = await server.transformRequest(stylesheetUrl);
    assertAssistantStyles(first, "initial load");

    const module = await server.moduleGraph.getModuleByUrl(stylesheetUrl);
    assert.ok(module?.transformResult, "the initial transform must populate Vite's module graph");
    server.moduleGraph.invalidateModule(module);
    assert.equal(module.transformResult, null, "invalidation must discard the cached transform");

    const reloaded = await server.transformRequest(stylesheetUrl);
    assert.notStrictEqual(reloaded, first, "the second request must compile again");
    assertAssistantStyles(reloaded, "after invalidation");
  } finally {
    try {
      await server?.close();
    } finally {
      // Only remove the unique directory this test created beneath the resolved system temp root.
      assert.equal(path.dirname(temporaryDirectory), temporaryRoot);
      assert.ok(path.basename(temporaryDirectory).startsWith("langame-assistant-css-"));
      assert.equal(fs.realpathSync(temporaryDirectory), temporaryDirectory);
      fs.rmSync(temporaryDirectory, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    }
  }
});
