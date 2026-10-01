const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { programCleanupDetails, emptyProgramCleanup } = require("./helpers/program-cleanup-fixture.cjs");

function deferred() {
  let resolve;
  const promise = new Promise((complete) => { resolve = complete; });
  return { promise, resolve };
}

function loadActions(api) {
  const filename = path.join(__dirname, "../src/hooks/useDesktopActions.ts");
  const dependencies = {
    react: {
      useCallback: (callback) => callback,
      useRef: (value) => ({ current: value }),
      useState: (initial) => {
        let value = typeof initial === "function" ? initial() : initial;
        return [value, (next) => { value = typeof next === "function" ? next(value) : next; }];
      }
    },
    "../api": api,
    "../app-state": { describeError: (error) => error.message },
    "../app-ui": { message: (key, params, extra) => ({ key, params, ...extra }), programCleanupDetails },
    "./useSteamCmdActions": { useSteamCmdActions: () => assert.fail("instance actions must not start library hooks") },
    "../installation-cancellation": {},
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": {},
    "../instance-panel-refresh": {},
    "../server-start-error": {},
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => {} }) }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    },
    window: {
      prompt: () => assert.fail("backup actions must not open native prompts"),
      confirm: () => assert.fail("backup actions must not open native confirmations")
    }
  }, { filename });
  const retirementFilename = path.join(__dirname, "../src/hooks/useInstanceRetirement.ts");
  const retirementExports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(retirementFilename, "utf8"), retirementFilename), {
    exports: retirementExports,
    require: (id) => {
      assert.equal(id, "react", `unexpected retirement dependency ${id}`);
      return dependencies.react;
    }
  }, { filename: retirementFilename });
  return (options) => {
    const retirement = retirementExports.useInstanceRetirement();
    let bootstrap = { state: { instances: [{ id: "a" }, { id: "b" }] } };
    const actions = exports.useInstanceActions({
      retirement,
      setBootstrap: (update) => { bootstrap = update(bootstrap); },
      ...options
    });
    return { ...actions, fixture: { retirement, getBootstrap: () => bootstrap } };
  };
}

const backup = { backup_id: "backup-a", backup_path: "backups/a", file_count: 1 };
const operations = [
  ["create", "createInstanceBackup", "handleCreateBackup", []],
  ["restore", "restoreInstanceBackup", "handleRestoreBackup", [backup.backup_id]],
  ["rename", "renameInstanceBackup", "handleRenameBackup", [backup, "Renamed backup"]],
  ["delete", "deleteInstanceBackup", "handleDeleteBackup", [backup]]
];

for (const [name, apiName, actionName, argumentsAfterId] of operations) {
  for (const [initialSelection, finalSelection] of [["a", "b"], ["a", null], ["a", "a"], ["b", "a"]]) {
    test(`${name} backup refresh follows the live selection (${initialSelection} -> ${finalSelection})`, async () => {
      const mutation = deferred();
      const listing = deferred();
      const listRequested = deferred();
      const originalA = [backup];
      const originalB = [{ ...backup, backup_id: "backup-b" }];
      const refreshedA = [{ ...backup, backup_id: "refreshed-a" }];
      let currentSelection = initialSelection;
      let cachedBackups = { a: originalA, b: originalB };
      let selectedBackups = cachedBackups[initialSelection];
      let selectedWrites = 0;
      const activity = [];
      const actions = loadActions({
        [apiName]: (instanceId) => {
          assert.equal(instanceId, "a");
          return mutation.promise;
        },
        listInstanceBackups: (instanceId) => {
          assert.equal(instanceId, "a");
          listRequested.resolve();
          return listing.promise;
        }
      })({
        selectedInstanceId: initialSelection,
        getCurrentInstanceId: () => currentSelection,
        instanceDetailsById: { a: { summary: { name: "Server A" } } },
        setInstanceDetailsById: () => {},
        setInstanceBackupsById: (update) => { cachedBackups = update(cachedBackups); },
        setSelectedInstanceBackups: (value) => { selectedBackups = value; selectedWrites += 1; },
        setActivity: (value) => activity.push(value)
      });

      const flight = actions[actionName]("a", ...argumentsAfterId);
      mutation.resolve({ ...backup, safeguard_backup_id: "safeguard-a" });
      await listRequested.promise;
      currentSelection = finalSelection;
      selectedBackups = finalSelection === null ? [] : cachedBackups[finalSelection];
      const selectedBeforeRefresh = selectedBackups;
      listing.resolve(refreshedA);
      await flight;

      assert.equal(cachedBackups.a, refreshedA, "refresh the operated instance's cache");
      assert.equal(cachedBackups.b, originalB, "preserve the other instance's cache");
      assert.equal(selectedWrites, finalSelection === "a" ? 1 : 0);
      assert.equal(selectedBackups, finalSelection === "a" ? refreshedA : selectedBeforeRefresh);
      assert.ok(!activity.at(-1).key.endsWith("Failed"), "the mutation must complete successfully");
    });
  }
}

test("rename receives the inline draft, trims it and allows clearing a custom name", async () => {
  const renamed = [];
  const activities = [];
  const actions = loadActions({
    renameInstanceBackup: async (...args) => { renamed.push(args); return backup; },
    listInstanceBackups: async () => [backup]
  })({
    selectedInstanceId: "a",
    getCurrentInstanceId: () => "a",
    instanceDetailsById: { a: { summary: { name: "Server A" } } },
    setInstanceDetailsById: () => {},
    setInstanceBackupsById: () => {},
    setSelectedInstanceBackups: () => {},
    setActivity: (value) => activities.push(value)
  });
  assert.equal(await actions.handleRenameBackup("a", backup, "  Named backup  "), true);
  assert.deepEqual(renamed[0], ["a", "backup-a", "Named backup"]);
  assert.equal(await actions.handleRenameBackup("a", { ...backup, display_name: "Named backup" }, "  "), true);
  assert.deepEqual(renamed[1], ["a", "backup-a", null]);
  assert.equal(await actions.handleRenameBackup("a", backup, "a"), true);
  assert.equal(renamed.length, 2, "saving the folder fallback must remain a no-op");
  assert.equal(activities.at(-1).key, "activity.instanceBackupRenamed");
});

test("rename reports failure to keep the editor and draft available", async () => {
  const activities = [];
  const actions = loadActions({ renameInstanceBackup: async () => { throw new Error("Name rejected"); } })({
    setActivity: (value) => activities.push(value)
  });
  assert.equal(await actions.handleRenameBackup("a", backup, "New name"), false);
  assert.equal(activities.at(-1).key, "activity.instanceBackupRenameFailed");
});

test("confirmed instance deletion reaches the API without requesting another native dialog", async () => {
  const deleted = [];
  const activities = [];
  let cleared = false;
  const actions = loadActions({
    deleteInstance: async (instanceId) => {
      deleted.push(instanceId);
      return { deleted_instance_root: "instances/a", instance_name: "Server A", preserved_external_saves_path: null,
        program_cleanup: emptyProgramCleanup() };
    }
  })({
    selectedInstanceId: "a",
    getCurrentInstanceId: () => "a",
    setActivity: (value) => activities.push(value),
    setInstanceDetailsById: () => {},
    setInstanceBackupsById: () => {},
    setInstanceRuntimesById: () => {},
    clearSelectedInstancePanel: () => { cleared = true; },
    setSelectedInstanceId: (value) => assert.equal(value, null),
    reloadBootstrap: async () => {}
  });
  await actions.handleDeleteInstance("a");
  assert.deepEqual(deleted, ["a"]);
  assert.equal(cleared, true);
  assert.equal(actions.fixture.retirement.isPending(), false);
  assert.equal(actions.fixture.getBootstrap().state.instances.map((instance) => instance.id).join(), "b");
  assert.equal(activities.at(-1).key, "activity.instanceDeleted");
});

test("archiving calls its own API and reports the retained external snapshot", async () => {
  const archived = [];
  const activities = [];
  let refreshed = 0;
  const actions = loadActions({
    deleteInstance: () => assert.fail("Archiving must not invoke permanent deletion"),
    archiveInstance: async (instanceId) => {
      archived.push(instanceId);
      return { archive_id: "archive-a", archived_instance_root: "instances/.trash/a", instance_name: "Server A",
        saves_archived_with_instance_root: false, preserved_external_saves_path: "external/world", external_saves_backup_id: "snapshot-a" };
    }
  })({ selectedInstanceId: null, getCurrentInstanceId: () => null, setActivity: (value) => activities.push(value),
    setInstanceDetailsById: () => {}, setInstanceBackupsById: () => {}, setInstanceRuntimesById: () => {},
    reloadBootstrap: async () => { refreshed++; } });
  await actions.handleArchiveInstance("a");
  assert.deepEqual(archived, ["a"]);
  assert.equal(refreshed, 1);
  assert.equal(activities.at(-1).key, "activity.instanceArchivedWithExternalSnapshot");
  assert.equal(activities.at(-1).params.savesPath, "external/world");
});

test("successful instance deletion reports retained program cleanup without changing into a deletion failure", async () => {
  const activities = [];
  const actions = loadActions({ deleteInstance: async () => ({ instance_name: "A", preserved_external_saves_path: null,
    program_cleanup: { removed_install_roots: ["fixture/obsolete"], preserved_data_paths: [],
      retained_installs: [{ install_root: "fixture/archive-source", reason: "archive_dependency" }] } }) })({
    getCurrentInstanceId: () => null, setActivity: (value) => activities.push(value),
    setInstanceDetailsById: () => {}, setInstanceBackupsById: () => {}, setInstanceRuntimesById: () => {},
    reloadBootstrap: async () => {}
  });
  await actions.handleDeleteInstance("a");
  assert.equal(activities.at(-1).key, "activity.completedWithProgramCleanup");
  assert.equal(activities.at(-1).tone, "warning");
  assert.equal(activities.at(-1).params.message, "activity.instanceDeleted");
  assert.equal(actions.fixture.getBootstrap().state.instances.map((instance) => instance.id).join(), "b");
});

test("failed deletion refreshes the workspace and preserves the failure for retry", async () => {
  let refreshed = 0;
  const activities = [];
  const failure = new Error("Deletion incomplete: file is in use");
  const actions = loadActions({ deleteInstance: async () => { throw failure; } })({
    setActivity: (value) => activities.push(value), reloadBootstrap: async () => { refreshed++; }
  });
  await assert.rejects(actions.handleDeleteInstance("a"), (error) => error === failure);
  assert.equal(refreshed, 1, "An incomplete deletion may already have left the active instance list");
  assert.equal(activities.at(-1).key, "activity.instanceDeleteFailed");
  assert.equal(activities.at(-1).params.message, failure.message);
  assert.equal(actions.fixture.retirement.isPending(), false, "Failed deletion must release the synchronous retirement owner");
});

for (const kind of ["Archive", "Delete"]) {
  test(`${kind.toLowerCase()} completion preserves a newer instance selection`, async () => {
    const operation = deferred();
    let calls = 0;
    let current = "a";
    let refreshed = 0;
    const actions = loadActions({ [kind === "Archive" ? "archiveInstance" : "deleteInstance"]: () => { calls++; return operation.promise; } })({
      selectedInstanceId: "a", getCurrentInstanceId: () => current,
      setActivity: () => {}, setInstanceDetailsById: () => {}, setInstanceBackupsById: () => {}, setInstanceRuntimesById: () => {},
      clearSelectedInstancePanel: () => assert.fail("The newer instance must stay selected"),
      setSelectedInstanceId: () => assert.fail("The newer selection must not be cleared"),
      reloadBootstrap: async () => { refreshed++; }
    });
    const pending = actions[`handle${kind}Instance`]("a");
    assert.equal(actions.fixture.retirement.isPending("a"), true);
    await actions[`handle${kind}Instance`]("a");
    assert.equal(calls, 1, "A second removal cannot bypass the synchronous retirement owner");
    current = "b";
    operation.resolve({ instance_name: "A", archived_instance_root: "archive/a", effective_saves_path: "a/saves",
      saves_archived_with_instance_root: true, preserved_external_saves_path: null, program_cleanup: emptyProgramCleanup() });
    await pending;
    assert.equal(refreshed, 1);
    assert.equal(actions.fixture.retirement.isPending(), false);
    assert.equal(actions.fixture.getBootstrap().state.instances.map((instance) => instance.id).join(), "b");
  });
}

test("a refresh failure does not turn successful permanent deletion into a failed deletion", async () => {
  const activities = [];
  const actions = loadActions({ deleteInstance: async () => ({ instance_name: "A", preserved_external_saves_path: null,
    program_cleanup: emptyProgramCleanup() }) })({
    getCurrentInstanceId: () => null, setActivity: (value) => activities.push(value),
    setInstanceDetailsById: () => {}, setInstanceBackupsById: () => {}, setInstanceRuntimesById: () => {},
    reloadBootstrap: async () => { throw new Error("Refresh unavailable"); }
  });
  await actions.handleDeleteInstance("a");
  assert.equal(activities.at(-1).key, "activity.instanceRemovalRefreshFailed");
  assert.equal(activities.at(-1).params.message, "activity.instanceDeleted");
  assert.equal(activities.at(-1).params.refreshMessage, "Refresh unavailable");
});
