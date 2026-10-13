const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const sourcePath = (...segments) => path.join(desktopRoot, "src", ...segments);
const readSource = (...segments) => fs.readFileSync(sourcePath(...segments), "utf8");

const routerSource = readSource("views", "AppViewRouter.tsx");
const systemSource = readSource("views", "SystemView.tsx");

test("the system dashboard owns all default paths and the SteamCMD lifecycle", () => {
  for (const key of ["games_root", "servers_root", "archives_root", "steamcmd_root"]) {
    assert.match(systemSource, new RegExp(`\\b${key}\\b`), `${key} should be rendered by SystemView`);
  }

  assert.match(systemSource, /appSettings/);
  assert.match(systemSource, /onPickDirectory/);
  assert.match(systemSource, /onSaveAppSettings/);
  assert.match(systemSource, /onEnsureSteamCmd/);
  assert.match(systemSource, /steamCmdStatus\?\.can_uninstall/);
  assert.equal(
    (systemSource.match(/disabled=\{props\.steamCmdBusy \|\| pathBusyKey !== null\}/g) ?? []).length,
    2,
    "SteamCMD install and uninstall should stay disabled while a root path is being saved"
  );

  assert.match(routerSource, /appSettings=\{props\.bootstrap\.state\.settings\}/);
  for (const propName of ["onPickDirectory", "onSaveAppSettings"]) {
    assert.match(routerSource, new RegExp(`${propName}=\\{props\\.${propName}\\}`));
  }
});
