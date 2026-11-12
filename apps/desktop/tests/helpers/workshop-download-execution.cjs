const fs = require("node:fs");
const { createRequire } = require("node:module");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");

/** Load the real orchestration while replacing only its native API boundary. */
exports.loadWorkshopDownloadExecution = (api, timers = { setInterval, clearInterval }) => {
  const filename = require.resolve("../../src/views/servers/workshop-download-execution.ts");
  const module = { exports: {} }, localRequire = createRequire(filename);
  new Function("require", "module", "exports", "setInterval", "clearInterval", transpileTypeScript(fs.readFileSync(filename, "utf8"), filename))(
    (id) => id === "../../api" ? api : localRequire(id), module, module.exports, timers.setInterval, timers.clearInterval);
  return module.exports.executeWorkshopDownload;
};
