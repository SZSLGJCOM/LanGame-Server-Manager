const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}

require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;

const desktopRoot = path.resolve(__dirname, "..");
const readSource = (...segments) => fs.readFileSync(path.join(desktopRoot, ...segments), "utf8");
const typesSource = readSource("src", "types.ts");
const statusSource = readSource("src", "steamcmd-ui.ts");
const systemViewSource = readSource("src", "views", "SystemView.tsx");
const mockSource = readSource("src", "api-mock.ts");
const {
  describeSteamCmdStatus,
  steamCmdDetailMessage,
  steamCmdSummaryMessage
} = require("../src/steamcmd-ui.ts");

test("SteamCMD status exposes ownership and backend-authorized uninstall", () => {
  assert.match(typesSource, /ownership:\s*SteamCmdOwnership/);
  assert.match(typesSource, /can_uninstall:\s*boolean/);
  assert.match(statusSource, /status\.ownership === "invalid"/);
  assert.doesNotMatch(statusSource, /legacy/i);
  assert.match(systemViewSource, /steamCmdStatus\?\.can_uninstall && \(/);
  assert.doesNotMatch(
    systemViewSource,
    /steamCmdStatus\?\.executable_exists && \([\s\S]{0,500}?onUninstallSteamCmd/,
    "an existing executable is not sufficient authority to delete its directory"
  );
});

test("an external SteamCMD installation stays detected without claiming management", () => {
  const status = {
    ownership: "external",
    executable_exists: true,
    ready: true,
    can_uninstall: false,
    root: "external-steamcmd",
    executable_path: "external-steamcmd/steamcmd.exe"
  };

  assert.deepEqual(describeSteamCmdStatus(status, (key) => key), {
    label: "system.steamCmdDetected",
    tone: "is-success"
  });
  assert.equal(steamCmdSummaryMessage(status).key, "activity.steamCmdDetected");
  assert.deepEqual(steamCmdDetailMessage(status), {
    key: "activity.steamCmdDetectedAt",
    params: { path: status.executable_path }
  });
  assert.equal(status.ownership, "external");
  assert.equal(status.can_uninstall, false);
});

test("a failed bootstrapper executable never claims that SteamCMD is ready", () => {
  for (const ownership of ["managed", "external"]) {
    const status = { ownership, executable_exists: true, ready: false, root: "steamcmd", executable_path: "steamcmd/steamcmd.exe" };
    assert.equal(describeSteamCmdStatus(status, (key) => key).label, "system.steamCmdNotReady");
    assert.equal(describeSteamCmdStatus(status, (key) => key).tone, "is-danger");
    assert.equal(steamCmdSummaryMessage(status).key, "system.steamCmdNotReady");
    assert.equal(steamCmdDetailMessage(status).key, "activity.steamCmdNotReadyAt");
  }
});

test("preview mode preserves the same SteamCMD ownership lifecycle", () => {
  assert.match(mockSource, /ownership:\s*"none",\s*\n\s*can_uninstall:\s*false/);
  assert.match(mockSource, /case "ensure_steamcmd_ready"[\s\S]*?ownership:\s*"managed"[\s\S]*?can_uninstall:\s*true/);
  assert.match(mockSource, /case "uninstall_steamcmd"[\s\S]*?ownership:\s*"none"[\s\S]*?can_uninstall:\s*false/);
  assert.match(mockSource, /steamCmdRootChanged[\s\S]*?ownership:\s*steamCmdRootChanged \? "none"/);
});
