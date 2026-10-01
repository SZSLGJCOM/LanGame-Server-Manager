const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const sourceRoot = path.join(__dirname, "..", "src");
const appSource = fs.readFileSync(path.join(sourceRoot, "App.tsx"), "utf8");
const shellSource = fs.readFileSync(path.join(sourceRoot, "components", "AppShell.tsx"), "utf8");
const actionsSource = fs.readFileSync(path.join(sourceRoot, "hooks", "useDesktopActions.ts"), "utf8");
const serversSource = fs.readFileSync(path.join(sourceRoot, "views", "ServersView.tsx"), "utf8");
const maintenanceSource = fs.readFileSync(path.join(sourceRoot, "views", "servers", "ServerMaintenanceWorkspace.tsx"), "utf8");
const backupTableSource = fs.readFileSync(path.join(sourceRoot, "views", "servers", "BackupTable.tsx"), "utf8");
const viewModelsSource = fs.readFileSync(path.join(sourceRoot, "view-models.ts"), "utf8");
const runtimeSurfaceSource = fs.readFileSync(
  path.join(sourceRoot, "views", "servers", "RuntimeSurfaceWorkbench.tsx"),
  "utf8"
);
const gmToolsSource = fs.readFileSync(
  path.join(sourceRoot, "views", "servers", "GMToolsWorkbench.tsx"),
  "utf8"
);
const playerCenterSource = fs.readFileSync(
  path.join(sourceRoot, "views", "servers", "PlayerCenterWorkbench.tsx"),
  "utf8"
);
const mockBootstrapSource = fs.readFileSync(
  path.join(sourceRoot, "api-mock", "bootstrap.ts"),
  "utf8"
);
const apiMockSource = fs.readFileSync(path.join(sourceRoot, "api-mock.ts"), "utf8");

test("desktop shell renders the latest activity message", () => {
  assert.match(appSource, /activityText=\{resolveUiMessage\(t, activity\)\}/);
  assert.match(shellSource, /className="shell-activity-bar"/);
  assert.match(shellSource, /role="status"/);
  assert.match(shellSource, /aria-live="polite"/);
  assert.match(shellSource, /\{props\.activityText\}/);
});

test("server start reports failure and prevents lifecycle broadcast", () => {
  assert.match(actionsSource, /instance\.start\.frontend_failed/);
  assert.match(actionsSource, /return true;[\s\S]*?catch \(error\)[\s\S]*?return false;/);
  assert.match(
    appSource,
    /const started = await handleStartServer\(instanceId, expectedWorldStart\);[\s\S]*?if \(started\) \{[\s\S]*?maybeRunLifecycleBroadcast\(instanceId, "startup"\)/
  );
});

test("server stop reports the backend error to persistent diagnostics", () => {
  assert.match(
    actionsSource,
    /handleStopServer[\s\S]*?instance\.stop\.frontend_failed[\s\S]*?\{ instanceId \}/
  );
});

test("runtime polling remains active while start and stop mutations run", () => {
  assert.match(
    appSource,
    /const runtimePollingEnabled = shouldPollRuntimeView && !runtimeAutoRefreshPaused/
  );
  assert.doesNotMatch(appSource, /runtimeActionInFlightCount/);
});

test("server start immediately selects the runtime console before awaiting the backend", () => {
  assert.match(
    serversSource,
    /if \(intent === "start"\) \{[\s\S]*?props\.onSelectInstance\(instanceId\);[\s\S]*?setActiveDetailTab\("runtime"\);[\s\S]*?props\.onWorkspaceSectionChange\("overview"\);[\s\S]*?await props\.onStart\(instanceId, expectedWorldStart\);/
  );
  assert.match(
    serversSource,
    /startupPending=\{runtimeActionsByInstanceId\[props\.selectedDetails\.summary\.id\] === "starting"\}/
  );
});

test("LanGameCMD follows the pending startup log before a runtime path is known", () => {
  assert.match(
    runtimeSurfaceSource,
    /const runtimeLogSubscriptionPath = startupPending \? "" : runtimeLogSourcePath/
  );
  assert.match(
    runtimeSurfaceSource,
    /if \(readOnly \|\| !isTauri\(\)\) \{[\s\S]*?followLatestPath: startupPending/
  );
  assert.doesNotMatch(runtimeSurfaceSource, /if \(!isTauri\(\) \|\| !runtimeLogSourcePath\)/);
});

test("partial runtime failures keep stop and surviving-process controls available", () => {
  assert.match(serversSource, /hasRunningProcess:\s*instanceHasRunningProcess\(props\.instance/);
  assert.match(runtimeSurfaceSource, /const instanceRunning = !readOnly && instanceHasRunningProcess\(props\.details\.summary, props\.details\.active_run\)/);
  assert.match(gmToolsSource, /instanceHasRunningProcess\(props\.details\.summary, props\.details\.active_run\)/);
  assert.match(gmToolsSource, /runtimeProcessKeyIsRunning\(props\.details\.active_run, commandPreview\.processKey\)/);
  assert.match(playerCenterSource, /runtimeProcessKeyIsRunning\(props\.details\.active_run, playerListAction\.process_key\)/);
  assert.match(serversSource, /!onlyRunning \|\| instanceHasRunningProcess\(instance\)/);
  assert.match(serversSource, /<ServerMaintenanceWorkspace active=\{activeDetailTab === "maintenance"\}\s+details=\{props\.selectedDetails\}/);
  assert.match(maintenanceSource, /const canRestoreBackup = !readOnly && !instanceHasRunningProcess\(props\.details\.summary, props\.details\.active_run\)/);
  assert.match(maintenanceSource, /<BackupTable[\s\S]*?canRestoreBackup=\{canRestoreBackup\}/);
  assert.match(backupTableSource, /disabled=\{props\.readOnly \|\| !props\.canRestoreBackup \|\| !props\.onRestoreBackup\}/);
  assert.match(viewModelsSource, /instances\.filter\(\(instance\) => instanceHasRunningProcess\(instance\)\)/);
});

test("runtime command targets disable exited shards while keeping running shards selectable", () => {
  assert.match(runtimeSurfaceSource, /disabled:\s*!runtimeProcessIsRunning\(process\)/);
  assert.match(runtimeSurfaceSource, /<option[^>]*disabled=\{target\.disabled\}/);
  assert.match(runtimeSurfaceSource, /const runtimeCommandEnabled = instanceRunning && !selectedTarget\?\.disabled/);
});

test("mock runtime exposes partial and terminal error instances for browser acceptance", () => {
  assert.match(mockBootstrapSource, /id: "srv-dst-partial-error"[\s\S]*?status: "Error"[\s\S]*?active_process_count: 1/);
  assert.match(mockBootstrapSource, /id: "srv-dst-terminal-error"[\s\S]*?status: "Error"[\s\S]*?active_process_count: 0/);
  assert.match(apiMockSource, /const cavesFailed = summary\.id === "srv-dst-partial-error"[\s\S]*?status: cavesFailed \? "error"/);
  assert.match(apiMockSource, /Number\(summary\.active_process_count\) > 0/);
});
