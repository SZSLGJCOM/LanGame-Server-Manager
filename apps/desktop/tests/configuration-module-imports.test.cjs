const assert = require("node:assert/strict");
const fs = require("node:fs");
const { registerHooks } = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { fileURLToPath, pathToFileURL } = require("node:url");
const ts = require("@typescript/typescript6");

const desktopRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(desktopRoot, "../..");
const entries = [
  ["ConfigurationField.tsx", "ConfigurationField", "function"],
  ["WorkshopIdListEditor.tsx", "WorkshopIdListEditor", "function"],
  ["module-registry.ts", "resolveSettingsModuleDefinition", "function"],
  ["modules/ark-asa.ts", "arkSurvivalAscendedSettingsDefinition", "object"],
  ["modules/ark-ase.ts", "arkSurvivalEvolvedSettingsDefinition", "object"],
  ["modules/scum.ts", "scumSettingsDefinition", "object"],
  ["modules/windrose.ts", "windroseSettingsDefinition", "object"]
];

// Preserve native ESM initialization; CommonJS and Vite SSR can hide this cycle.
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (!specifier.startsWith(".") && !specifier.startsWith("file:")) {
      return nextResolve(specifier, context);
    }
    const url = new URL(specifier, context.parentURL);
    const filename = fileURLToPath(url);
    const resolved = [filename, ...[".ts", ".tsx", ".js", ".json", "/index.ts", "/index.tsx"]
      .map((extension) => `${filename}${extension}`)].find((candidate) => fs.statSync(candidate, { throwIfNoEntry: false })?.isFile());
    if (!resolved || !resolved.startsWith(`${repositoryRoot}${path.sep}`)) {
      return nextResolve(specifier, context);
    }
    const target = pathToFileURL(resolved);
    target.search = url.search;
    // Each entry evaluates a fresh graph, so an earlier successful import cannot mask its order.
    const entry = context.parentURL && new URL(context.parentURL).searchParams.get("configuration-entry");
    if (entry) target.searchParams.set("configuration-entry", entry);
    return { url: target.href, shortCircuit: true };
  },
  load(url, context, nextLoad) {
    if (!url.startsWith("file:")) return nextLoad(url, context);
    const filename = fileURLToPath(url);
    if (!filename.startsWith(`${repositoryRoot}${path.sep}`) || filename.includes(`${path.sep}node_modules${path.sep}`)) {
      return nextLoad(url, context);
    }
    const extension = path.extname(filename);
    if (extension === ".json") {
      return { format: "module", source: `export default ${fs.readFileSync(filename, "utf8")};`, shortCircuit: true };
    }
    if ([".css", ".png", ".svg"].includes(extension)) {
      return { format: "module", source: `export default ${JSON.stringify(filename)};`, shortCircuit: true };
    }
    if (extension !== ".ts" && extension !== ".tsx") return nextLoad(url, context);
    const result = ts.transpileModule(fs.readFileSync(filename, "utf8"), {
      fileName: filename,
      reportDiagnostics: true,
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        jsx: ts.JsxEmit.ReactJSX,
        module: ts.ModuleKind.ESNext
      }
    });
    const errors = result.diagnostics
      .filter((diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error)
      .map((diagnostic) => {
        const position = diagnostic.file && diagnostic.start !== undefined
          ? diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start) : null;
        const location = position ? `${filename}:${position.line + 1}:${position.character + 1}` : filename;
        return `${location}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`;
      });
    assert.deepEqual(errors, [], `TypeScript fixture must compile: ${filename}`);
    const source = result.outputText;
    return { format: "module", source: `import.meta.env = { DEV: true, MODE: "development" };\n${source}`, shortCircuit: true };
  }
});
test.after(() => hooks.deregister());

for (const [filename, exportName, exportType] of entries) {
  test(`configuration modules initialize with ${filename} as the ESM entry`, { timeout: 30000 }, async () => {
    const entry = pathToFileURL(path.join(desktopRoot, "src/views/settings", filename));
    entry.searchParams.set("configuration-entry", filename);
    const loaded = await import(entry.href);
    assert.equal(typeof loaded[exportName], exportType);
    const registryUrl = pathToFileURL(path.join(desktopRoot, "src/views/settings/module-registry.ts"));
    registryUrl.search = entry.search;
    const registry = await import(registryUrl.href);
    for (const moduleId of ["arksurvivalascended", "arksurvivalevolved", "scum", "windrose"]) {
      assert.equal(registry.resolveSettingsModuleDefinition(moduleId)?.id, moduleId);
    }
  });
}
