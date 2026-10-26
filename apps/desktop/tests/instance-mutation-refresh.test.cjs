const assert = require("node:assert/strict");
const test = require("node:test");
const { instanceActionFixture } = require("./helpers/instance-action-fixture.cjs");

const backup = { backup_id: "backup-a", backup_path: "backups/a", file_count: 1, safeguard_backup_id: "safeguard-a" };
const start = { summary: { id: "a", name: "Server A" }, run_id: 2, warnings: [] };
const cases = [
  { name: "window suppression", api: "suppressInstanceRuntimeWindows", result: { visible_window_count_before: 1,
    remaining_visible_window_count: 0, suppressed_window_count: 1 },
    run: (actions) => actions.handleSuppressRuntimeWindows("a"),
    failureKey: "activity.runtimeWindowsSuppressFailed", refresh: "panel" },
  { name: "server creation", api: "createInstance", result: { summary: start.summary },
    run: (actions) => actions.handleCreateServer({ module_id: "valheim", name: "Server A" }),
    failureKey: "activity.createServerFailed", refresh: "bootstrap" },
  { name: "server start", api: "startInstance", result: start,
    run: (actions) => actions.handleStartServer("a"), success: true, failure: false,
    failureKey: "activity.startServerFailed", refresh: "bootstrap" },
  { name: "server stop", api: "stopInstance", result: undefined,
    run: (actions) => actions.handleStopServer("a"),
    failureKey: "activity.stopServerFailed", refresh: "bootstrap" },
  { name: "backup creation", api: "createInstanceBackup", result: backup,
    run: (actions) => actions.handleCreateBackup("a"),
    failureKey: "activity.instanceBackupFailed", refresh: "backups" },
  { name: "backup restoration", api: "restoreInstanceBackup", result: backup,
    run: (actions) => actions.handleRestoreBackup("a", backup.backup_id),
    failureKey: "activity.instanceBackupRestoreFailed", refresh: "backups" },
  { name: "backup rename", api: "renameInstanceBackup", result: backup,
    run: (actions) => actions.handleRenameBackup("a", backup, "New name"), success: true, failure: false,
    failureKey: "activity.instanceBackupRenameFailed", refresh: "backups" },
  { name: "backup deletion", api: "deleteInstanceBackup", result: backup,
    run: (actions) => actions.handleDeleteBackup("a", backup),
    failureKey: "activity.instanceBackupDeleteFailed", refresh: "backups" }
];

for (const operation of cases) {
  test(`${operation.name}: view failure cannot replace the successful operation result`, async () => {
    const refreshError = new Error("View unavailable");
    const h = instanceActionFixture({
      api: { [operation.api]: async () => operation.result,
        ...(operation.refresh === "panel" ? { readInstanceRuntimeWindowSnapshot: async () => { throw refreshError; } } : {}),
        ...(operation.refresh === "backups" ? { listInstanceBackups: async () => { throw refreshError; } } : {}) },
      reloadBootstrap: operation.refresh === "bootstrap" ? async () => { throw refreshError; } : undefined
    });
    assert.equal(await operation.run(h.actions), operation.success);
    const activity = h.state.activity.at(-1);
    assert.equal(activity.key, "activity.operationRefreshFailed");
    assert.match(activity.params.refreshMessage, /View unavailable/);
    assert.equal(h.state.activity.some((value) => value.key === operation.failureKey), false);
    assert.equal(h.state.calls.filter((call) => call.name === operation.api).length, 1, "refresh must never repeat the mutation");
  });

  test(`${operation.name}: actual operation rejection keeps its original failure`, async () => {
    const h = instanceActionFixture({ api: {
      [operation.api]: async () => { throw new Error("Operation rejected"); }
    } });
    assert.equal(await operation.run(h.actions), operation.failure);
    assert.equal(h.state.activity.at(-1).key, operation.failureKey);
    assert.equal(h.state.calls.filter((call) => call.name === operation.api).length, 1);
    assert.equal(h.state.calls.some((call) => call.name === "readInstanceDetails" || call.name === "listInstanceBackups"), false);
    assert.equal(h.state.bootstrapRefreshes, 0);
  });
}

test("backup acknowledgement survives a later details read failure", async () => {
  const h = instanceActionFixture({ api: {
    createInstanceBackup: async () => backup,
    readInstanceDetails: async () => { throw new Error("Details unavailable"); }
  } });
  await h.actions.handleCreateBackup("a");
  const activity = h.state.activity.at(-1);
  assert.equal(activity.key, "activity.operationRefreshFailed");
  assert.match(activity.params.refreshMessage, /Details unavailable/);
  assert.equal(h.state.calls.filter((call) => call.name === "createInstanceBackup").length, 1);
});

test("a start warning remains available when its view refresh fails", async () => {
  const warning = { code: "program_update_using_installed", provider: null, reason: null,
    item_ids: [], message: "Using installed program", log_path: null, map_key: null, http_status: null };
  const h = instanceActionFixture({ api: { startInstance: async () => ({ ...start, warnings: [warning] }) },
    reloadBootstrap: async () => { throw new Error("Inventory unavailable"); } });
  assert.equal(await h.actions.handleStartServer("a"), true);
  const activity = h.state.activity.at(-1);
  assert.equal(activity.key, "activity.operationRefreshFailed");
  assert.equal(activity.runtimeStartNotice.warnings[0], warning);
});
