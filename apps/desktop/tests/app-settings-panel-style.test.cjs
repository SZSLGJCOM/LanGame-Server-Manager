const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const sourcePath = (...segments) => path.join(desktopRoot, "src", ...segments);
const readSource = (...segments) => fs.readFileSync(sourcePath(...segments), "utf8");

const headerSource = readSource("components", "AppHeader.tsx");
const routerSource = readSource("views", "AppViewRouter.tsx");
const systemSource = readSource("views", "SystemView.tsx");
const assistantCapsuleSource = readSource("components", "AssistantCapsule.tsx");
const assistantPanelSource = readSource("components", "AssistantPanel.tsx");
const aiSettingsSource = readSource("components", "AppAiSettingsCard.tsx");

const retiredSettingsComponents = [
  sourcePath("components", "AppSettingsPanel.tsx"),
  sourcePath("components", "AppSettingsPanelDialog.tsx")
];

test("the header no longer exposes the avatar settings entry", () => {
  assert.doesNotMatch(headerSource, /AppSettingsPanel/);
  assert.doesNotMatch(headerSource, /app-settings-trigger(?:-avatar)?/);
  assert.doesNotMatch(headerSource, /settingsPanelOpen|onSettingsPanelOpenChange/);
});

test("the retired application settings page is removed instead of kept as dead code", () => {
  for (const componentPath of retiredSettingsComponents) {
    assert.equal(
      fs.existsSync(componentPath),
      false,
      `${path.basename(componentPath)} should be removed with the application settings page`
    );
  }
});

test("the system dashboard owns all default paths and the SteamCMD lifecycle", () => {
  for (const key of ["games_root", "servers_root", "archives_root", "steamcmd_root"]) {
    assert.match(systemSource, new RegExp(`\\b${key}\\b`), `${key} should be rendered by SystemView`);
  }

  assert.match(systemSource, /appSettings/);
  assert.match(systemSource, /onPickDirectory/);
  assert.match(systemSource, /onSaveAppSettings/);
  assert.match(
    systemSource,
    /props\.onPickDirectory\([\s\S]{0,1600}?props\.onSaveAppSettings\(/,
    "selecting a directory should persist the complete path settings object"
  );
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

test("AI settings open from the assistant panel header and stay inside the island", () => {
  assert.match(assistantPanelSource, /import \{ AppAiSettingsCard \} from "\.\/AppAiSettingsCard"/);
  assert.match(
    assistantPanelSource,
    /assistant-panel-controls[\s\S]{0,1200}?<ShellIcon name="settings"/,
    "the AI settings button should live with the assistant panel header controls"
  );
  assert.match(assistantPanelSource, /<AppAiSettingsCard/);
  assert.match(assistantPanelSource, /onSaveAiSettings/);
  assert.match(assistantPanelSource, /onClearAiSecret/);

  assert.match(aiSettingsSource, /app-settings-card--ai/);
  assert.match(aiSettingsSource, /ai-settings-grid/);
  assert.match(aiSettingsSource, /app-settings-form/);
});

test("the assistant dialog traps keyboard focus and restores the island trigger", () => {
  assert.match(assistantCapsuleSource, /panelRef/);
  assert.match(assistantCapsuleSource, /event\.key !== "Tab"/);
  assert.match(assistantCapsuleSource, /containAssistantFocus\(event, surfaceRef\.current, document\.activeElement\)/);
  assert.match(assistantCapsuleSource, /buttonRef\.current\?\.focus\(\{ preventScroll: true \}\)/);
  assert.match(assistantCapsuleSource, /onClose=\{closePanel\}/);
  assert.match(assistantPanelSource, /aria-modal=\{props\.embedded \? undefined : true\}/);
});
