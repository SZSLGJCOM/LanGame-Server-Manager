const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const desktopRoot = path.join(root, "apps", "desktop");
const settingsDir = path.join(desktopRoot, "src", "views", "settings");
const { transpileTypeScript } = require(path.join(
  desktopRoot,
  "scripts",
  "typescript_source_tools.cjs"
));

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const { listSettingsModuleIds } = require(path.join(settingsDir, "module-registry.ts"));
process.stdout.write(`${JSON.stringify(listSettingsModuleIds())}\n`);
