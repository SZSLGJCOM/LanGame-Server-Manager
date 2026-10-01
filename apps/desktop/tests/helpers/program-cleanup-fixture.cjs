const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");

const filename = path.join(__dirname, "../../src/app-ui.ts");
const exportsObject = {};
vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
  exports: exportsObject,
  require: (name) => {
    assert.equal(name, "./desktop-error-message");
    return { formatDesktopError: (_t, value) => value };
  }
}, { filename });

module.exports = {
  ...exportsObject,
  emptyProgramCleanup: () => ({ removed_install_roots: [], preserved_data_paths: [], retained_installs: [] })
};
