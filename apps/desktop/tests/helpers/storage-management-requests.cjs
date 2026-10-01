const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");

function loadStorageManagementRequests(localePreference) {
  const modules = new Map();
  function load(name) {
    if (modules.has(name)) return modules.get(name);
    const filename = path.join(__dirname, "../../src", `${name}.ts`);
    const exports = {};
    modules.set(name, exports);
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
      exports,
      require(id) {
        if (id === "./locale-preference" && localePreference) return localePreference;
        if (["./i18n-config", "./locale-preference"].includes(id)) return load(id.slice(2));
        throw new Error(`Unexpected storage coordinator dependency: ${id}`);
      }
    }, { filename });
    return exports;
  }
  return load("storage-management-requests");
}

module.exports = { loadStorageManagementRequests };
