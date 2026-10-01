const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");

function loadDesktopExitModule(core, listen = async () => () => {}) {
  const filename = path.resolve(__dirname, "../../src/desktop-exit-lifecycle.ts");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, Error,
    require(id) {
      if (id === "@tauri-apps/api/core") return core;
      if (id === "@tauri-apps/api/event") return { listen };
      throw new Error(`Unexpected desktop exit dependency: ${id}`);
    }
  }, { filename });
  return exports;
}

module.exports = { loadDesktopExitModule };
