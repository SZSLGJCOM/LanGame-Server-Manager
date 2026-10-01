const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { moduleRequiresSteamCmd, steamCmdInstallBlockReason } = require("../src/module-installation-dependency.ts");

test("Steam depot acquisition requires SteamCMD while independent providers do not", () => {
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: 2394010 }, {}), true);
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: 2394010 }, null), true);
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: null }, { source: "minecraft_java" }), false);
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: null }, { download_url_windows: "https://example.test/server.zip" }), false);
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: 2394010 }, { download_url_windows: "https://example.test/server.zip" }), false,
    "a direct payload takes priority even when a module has a Steam AppID");
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: 2394010 }, { source: "minecraft_java" }), false);
  assert.equal(moduleRequiresSteamCmd({ steam_app_id: 0 }, {}), false);
  assert.equal(moduleRequiresSteamCmd({}, null), false);
});

test("availability requires completed preparation and pauses during dependency changes", () => {
  assert.equal(steamCmdInstallBlockReason(null, false), "unknown");
  assert.equal(steamCmdInstallBlockReason(null, true), "busy");
  assert.equal(steamCmdInstallBlockReason({ ready: false, executable_exists: false }, false), "missing");
  assert.equal(steamCmdInstallBlockReason({ ready: false, executable_exists: true }, false), "not_ready");
  assert.equal(steamCmdInstallBlockReason({ ready: true, executable_exists: true }, true), "busy");
  assert.equal(steamCmdInstallBlockReason({ ready: true, executable_exists: true }, false), null);
  assert.equal(steamCmdInstallBlockReason({ ready: true, executable_exists: false }, false), "missing",
    "an inconsistent stale readiness flag cannot enable installation without its executable");
});
