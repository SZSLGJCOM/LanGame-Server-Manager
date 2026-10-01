const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");
const sourcePath = (...segments) => path.join(desktopRoot, "src", ...segments);
const readSource = (...segments) => fs.readFileSync(sourcePath(...segments), "utf8");

const appSource = readSource("App.tsx");
const shellSource = readSource("components", "AppShell.tsx");
const headerSource = readSource("components", "AppHeader.tsx");
const assistantCapsuleSource = readSource("components", "AssistantCapsule.tsx");
const assistantPanelSource = readSource("components", "AssistantPanel.tsx");
const routerSource = readSource("views", "AppViewRouter.tsx");
const systemSource = readSource("views", "SystemView.tsx");

test("the obsolete settings update card is removed", () => {
  assert.equal(
    fs.existsSync(sourcePath("components", "AppUpdateSettingsCard.tsx")),
    false,
    "Application updates should use the shared automatic prompt"
  );
});

test("application update state bypasses the system dashboard", () => {
  for (const source of [routerSource, systemSource]) {
    assert.doesNotMatch(source, /AppUpdateState|appUpdateState|onCheckAppUpdate|onInstallAppUpdate/);
  }
});

test("the shell owns the update prompt without a header or assistant entry", () => {
  for (const source of [appSource, shellSource]) {
    assert.match(source, /appUpdateState/);
    assert.match(source, /onInstallAppUpdate/);
    assert.match(source, /appUpdatesEnabled/);
  }
  for (const source of [headerSource, assistantCapsuleSource, assistantPanelSource]) {
    assert.doesNotMatch(source, /AppUpdateState|appUpdateState|onCheckAppUpdate|onInstallAppUpdate|assistant-update/);
  }
  assert.match(shellSource, /<AppUpdatePrompt/);
  assert.match(shellSource, /appUpdateReleaseUrl\(props\.appUpdateState\.availableVersion\)/);
  assert.match(shellSource, /await openExternalUrl\(url\)/);
  assert.equal(fs.existsSync(sourcePath("components", "AppUpdateControl.tsx")), false);
  assert.equal(fs.existsSync(sourcePath("components", "AssistantUpdateNotice.tsx")), false);
});

test("the desktop checks for application updates automatically after startup", () => {
  const checksDuringBootstrap =
    /handleBootstrapped[\s\S]{0,1000}?(?:void\s+)?handleCheckAppUpdate\([^)]*\)/.test(appSource);
  const checksFromStartupEffect =
    /useEffect\(\(\)\s*=>\s*\{[\s\S]{0,1200}?(?:runAutomaticAppUpdateCheck|handleCheckAppUpdate|checkAppUpdate)\(\)/.test(appSource);

  assert.equal(
    checksDuringBootstrap || checksFromStartupEffect,
    true,
    "App should perform a one-shot update check during desktop startup"
  );
  assert.match(
    appSource,
    /const DESKTOP_UPDATES_ENABLED = import\.meta\.env\.VITE_LANGAME_DESKTOP_UPDATES_ENABLED === "true"/,
    "pre-release builds should keep the updater dormant"
  );
  assert.match(
    appSource,
    /useEffect\(\(\)\s*=>\s*\{[\s\S]{0,300}?!DESKTOP_UPDATES_ENABLED[\s\S]{0,120}?!isTauri\(\)/,
    "only release desktop builds should invoke the updater"
  );
  assert.match(appSource, /setInterval\(runAutomaticAppUpdateCheck, APP_UPDATE_CHECK_INTERVAL_MS\)/);
  assert.match(appSource, /clearInterval\(intervalId\)/, "the periodic update check must have a managed lifetime");
  assert.match(
    appSource,
    /handleBootstrapMetadata[\s\S]{0,500}?payload\.appVersion[\s\S]{0,200}?setAppUpdateChecksReady\(true\)/,
    "the updater should start once the desktop app version is known"
  );
  assert.match(
    appSource,
    /handleCheckAppUpdate[\s\S]{0,300}?!DESKTOP_UPDATES_ENABLED/,
    "manual checks must share the release-build gate"
  );
  assert.doesNotMatch(
    appSource,
    /if \([^)]*bootstrapInitializationError[^)]*\)[\s\S]{0,300}?runAutomaticAppUpdateCheck/,
    "storage migration failures must not suppress application updates"
  );
  assert.match(
    appSource,
    /app\.update\.auto_check_failed[\s\S]{0,300}?failAppUpdateState/,
    "a failed background check should retain error state for available recovery actions"
  );
});

test("an update install becomes busy before the updater command can be invoked twice", () => {
  assert.match(appSource, /appUpdateInstallPendingRef\.current\s*\|\|\s*appUpdateState\.status\s*!==\s*"available"/);
  assert.match(
    appSource,
    /appUpdateInstallPendingRef\.current\s*=\s*true[\s\S]{0,500}?status:\s*"downloading"[\s\S]{0,500}?await installAppUpdate\(/,
    "the updater should become busy before awaiting the first updater event"
  );
});
