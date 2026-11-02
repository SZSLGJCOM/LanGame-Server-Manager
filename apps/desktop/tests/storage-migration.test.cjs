const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { SingleFlightPoller } = require("../src/domain/single-flight-poller.ts");
const migrationTypes = require("../src/storage-migration-types.ts");
function load(file, dependencies, environment = {}) {
  const filename = path.join(__dirname, "../src", file);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { exports, console, navigator: { platform: "Win32" },
    require: name => { assert.ok(name in dependencies, name); return dependencies[name]; }, ...environment }, { filename });
  return exports;
}
function api(native = true, invoke = async () => null, platform = "Win32") {
  return load("api-storage-migration.ts", { "@tauri-apps/api/core": { isTauri: () => native },
    "./api-transport": { invokeOrMock: invoke } }, { navigator: { platform } });
}
const { createStorageMigrationController } = load("hooks/useStorageMigration.ts", {
  react: {}, "../api-storage-migration": { storageMigrationApi: {} },
  "../domain/single-flight-poller": { SingleFlightPoller }, "../storage-migration-types": migrationTypes,
  "../views/system/app-path-settings-draft": require("../src/views/system/app-path-settings-draft.ts")
});
const input = { games_root: "D:/games", servers_root: "D:/instances", archives_root: "D:/instances/.trash", steamcmd_root: "D:/steamcmd" };
const status = (phase = "ready", extra = {}) => ({ operation_id: "migration-one", phase, input,
  roots: [{ source: "E:/instances", target: "D:/instances" }], total_files: 3, total_bytes: 24,
  completed_files: 0, completed_bytes: 0, reused_files: 0, conflicts: [], conflict_count: 0,
  can_cancel: !["applying", "cleaning", "completed", "cancelled", "failed"].includes(phase), recovery_required: false, error: null, cleanup_warnings: [], preserved_destinations: [], ...extra });
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const flush = () => new Promise(resolve => setImmediate(resolve));
function dismissalStorage() {
  const values = new Map();
  return { values, getItem: key => values.get(key) ?? null, setItem: (key, value) => { values.set(key, value); } };
}
function setup(storage = dismissalStorage()) {
  const reads = [], calls = [], timers = new Map(), completions = [];
  let nextTimer = 0;
  const callbacks = { onCompleted: async value => { completions.push(value); } };
  const controller = createStorageMigrationController({
    api: { available: () => true, read: () => { const result = deferred(); reads.push(result); return result.promise; },
      ...Object.fromEntries(["preview", "execute", "cancel"].map(command => [command, argument => {
        const result = deferred(); calls.push({ command, argument, ...result }); return result.promise;
      }])) },
    dismissalStorage: storage,
    onCompleted: value => callbacks.onCompleted(value),
    schedule: callback => { const id = ++nextTimer; timers.set(id, callback); return id; }, cancelTimer: id => timers.delete(id)
  });
  const tick = () => { const next = timers.entries().next().value; assert.ok(next, "A status read must remain scheduled"); timers.delete(next[0]); next[1](); };
  controller.connect();
  return { controller, reads, calls, timers, completions, callbacks, tick, state: controller.getSnapshot };
}

test("migration API validates snapshots and never sends local file operations in a browser preview", async () => {
  let commands = 0;
  const unavailable = api(false, async () => { commands++; });
  assert.equal(unavailable.storageMigrationApi.available(), false);
  await assert.rejects(unavailable.storageMigrationApi.preview(input), /only on the local Windows LGSM desktop host/);
  assert.equal(commands, 0);
  const nonWindows = api(true, async () => { commands++; }, "Linux x86_64").storageMigrationApi;
  assert.equal(nonWindows.available(), false);
  await assert.rejects(nonWindows.preview(input), /local Windows/);
  assert.equal(commands, 0);
  const { parseStorageMigrationStatus: parse } = api();
  assert.equal(parse(status()).phase, "ready");
  assert.equal(parse(status("failed", { recovery_required: true })).recovery_required, true);
  for (const change of [{ phase: "invented" }, { operation_id: "" }, { total_bytes: -1 }, { completed_files: NaN },
    { recovery_required: undefined }, { recovery_required: null }, { recovery_required: "false" },
    { conflicts: [{ source: "E:/one", target: "D:/one", message: "Unsafe target" }], conflict_count: 0 }, { input: { ...input, archives_root: "" } }]) {
    assert.throws(() => parse(status("ready", change)), /Invalid storage migration status/);
  }
  assert.throws(() => parse(status(), "another-operation"), /Invalid storage migration status/);
});

test("migration API uses the fixed four commands and preserves the operation identity", async () => {
  const calls = [];
  const { storageMigrationApi: transport } = api(true, async (command, args) => { calls.push({ command, args }); return status(); });
  await transport.preview(input); await transport.read(); await transport.execute("migration-one"); await transport.cancel("migration-one");
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { command: "preview_app_storage_migration", args: { input } }, { command: "read_app_storage_migration" },
    { command: "execute_app_storage_migration", args: { operationId: "migration-one" } },
    { command: "cancel_app_storage_migration", args: { operationId: "migration-one" } }
  ]);
});

test("migration API preserves target backups and rejects malformed backup locations", () => {
  const { parseStorageMigrationStatus: parse } = api();
  const backups = [{ original_path: "D:/instances", backup_path: "D:/instances.previous-contents" }];
  assert.deepEqual(JSON.parse(JSON.stringify(parse(status("completed", { preserved_destinations: backups })).preserved_destinations)), backups);
  for (const preserved_destinations of [undefined, null, {}, [null], [{ original_path: "", backup_path: "D:/kept" }],
    [{ original_path: "D:/instances", backup_path: " " }]]) {
    assert.throws(() => parse(status("completed", { preserved_destinations })), /Invalid storage migration status/);
  }
});

test("preview scans before execution and conflicts prevent a mutation", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const preview = run.controller.migrate(input);
  assert.equal(run.state().pending, "preview");
  run.calls[0].resolve(status("checking")); await preview;
  assert.equal(run.state().status.phase, "checking");
  await run.controller.migrate(input);
  assert.equal(run.calls.length, 1);
  run.reads[1].resolve(status("ready", { conflict_count: 1, conflicts: [{ source: "E:/one", target: "D:/one", message: "Unsafe target" }] })); await flush();
  assert.equal(run.calls.length, 1);
  assert.equal(run.timers.size, 0);
  run.controller.disconnect();
});

test("execute submits once and confirmed completion survives cleanup and refresh warnings", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const refresh = deferred(); run.callbacks.onCompleted = value => { run.completions.push(value); return refresh.promise; };
  const preview = run.controller.migrate(input); run.calls[0].resolve(status()); await preview;
  await run.controller.migrate(input); assert.equal(run.calls.length, 2);
  assert.equal(run.calls[1].command, "execute");
  run.calls[1].resolve(status("copying")); await flush();
  run.reads[1].resolve(status("completed", { completed_files: 3, completed_bytes: 24, cleanup_warnings: ["Old file retained: E:/locked"] })); await flush();
  assert.equal(run.state().status.phase, "completed"); assert.equal(run.state().refreshing, true);
  refresh.reject(new Error("bootstrap temporarily unavailable")); await flush();
  assert.equal(run.state().status.phase, "completed");
  assert.match(run.state().refreshError, /bootstrap temporarily unavailable/);
  assert.equal(run.state().status.cleanup_warnings[0], "Old file retained: E:/locked");
  run.callbacks.onCompleted = async value => { run.completions.push(value); };
  await run.controller.refresh();
  assert.equal(run.state().refreshError, null); assert.equal(run.calls.length, 2);
  run.controller.disconnect();
});

test("a lost execution receipt observes the same operation without resubmitting", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const preview = run.controller.migrate(input); run.calls[0].resolve(status()); await preview;
  assert.equal(run.calls[1].command, "execute");
  run.calls[1].reject(new Error("Runtime response timed out; the operation may still be running")); await flush();
  assert.equal(run.state().unconfirmed, true);
  run.reads[1].resolve(null); await flush();
  assert.equal(run.state().unconfirmed, true);
  await run.controller.migrate(input); assert.equal(run.calls.length, 2);
  run.tick(); run.reads[2].resolve(status("copying")); await flush();
  assert.equal(run.state().unconfirmed, false); assert.equal(run.state().status.phase, "copying");
  assert.equal(run.calls.length, 2);
  run.controller.disconnect();
});

test("cancel acknowledgement keeps observing until native cancellation and preserves the submitted draft", async () => {
  const run = setup(); run.reads[0].resolve(status("copying")); await flush();
  const cancelling = run.controller.cancel(); run.calls[0].resolve(status("copying")); await cancelling;
  assert.equal(run.state().cancelRequested, true); assert.equal(run.state().status.phase, "copying");
  await run.controller.cancel(); assert.equal(run.calls.length, 1);
  run.reads[1].resolve(status("cancelled")); await flush();
  assert.equal(run.state().status.phase, "cancelled"); assert.equal(run.state().cancelRequested, false);
  assert.deepEqual(run.state().status.input, input); assert.equal(run.completions.length, 0);
  run.controller.disconnect();
});

test("recovery observes a previous migration and ignores late reads after disconnect", async () => {
  const run = setup(); run.reads[0].resolve(status("verifying")); await flush();
  assert.equal(run.state().status.phase, "verifying"); assert.equal(run.calls.length, 0);
  run.tick(); run.controller.disconnect();
  run.reads[1].resolve(status("completed")); await flush();
  assert.equal(run.state().status.phase, "verifying"); assert.equal(run.completions.length, 0); assert.equal(run.timers.size, 0);
  run.controller.connect(); run.reads[2].resolve(status("completed")); await flush();
  assert.equal(run.state().status.phase, "completed"); assert.equal(run.completions.length, 1); assert.equal(run.calls.length, 0);
  run.controller.disconnect();
});

test("cancelling a ready preview continues reading until the cancellation is confirmed", async () => {
  const run = setup(); run.reads[0].resolve(status()); await flush();
  const cancelling = run.controller.cancel(); run.calls[0].resolve(status()); await cancelling;
  assert.equal(run.state().cancelRequested, true); assert.equal(run.reads.length, 2);
  await run.controller.migrate(input);
  assert.equal(run.calls.length, 1, "Cancellation must settle before executing or replacing a ready preview");
  run.reads[1].resolve(status()); await flush();
  assert.equal(run.state().cancelRequested, true); assert.equal(run.timers.size, 1);
  run.tick(); run.reads[2].resolve(status("cancelled")); await flush();
  assert.equal(run.state().cancelRequested, false); assert.equal(run.timers.size, 0);
  run.controller.disconnect();
});

test("a lost cancellation receipt keeps the ready operation locked until its terminal result", async () => {
  const run = setup(); run.reads[0].resolve(status()); await flush();
  const cancelling = run.controller.cancel(); run.calls[0].reject(new Error("Cancellation receipt timed out")); await cancelling;
  run.reads[1].resolve(status()); await flush();
  assert.equal(run.state().cancelRequested, true);
  await run.controller.migrate(input);
  assert.equal(run.calls.length, 1);
  run.tick(); run.reads[2].resolve(status("cancelled")); await flush();
  assert.equal(run.state().cancelRequested, false); assert.equal(run.state().unconfirmed, false);
  run.controller.disconnect();
});

test("a late cancellation receipt cannot replace a confirmed completed operation", async () => {
  const run = setup(); run.reads[0].resolve(status("copying")); await flush();
  const cancelling = run.controller.cancel();
  run.tick(); run.reads[1].resolve(status("completed")); await flush();
  assert.equal(run.completions.length, 1);
  run.calls[0].resolve(status("cancelled")); await cancelling;
  assert.equal(run.state().status.phase, "completed"); assert.equal(run.completions.length, 1);
  run.controller.disconnect();
});

test("a late cancellation error cannot turn a confirmed completion into an unknown outcome", async () => {
  const run = setup(); run.reads[0].resolve(status("copying")); await flush();
  const cancelling = run.controller.cancel();
  run.tick(); run.reads[1].resolve(status("completed")); await flush();
  run.calls[0].reject(new Error("Cancellation receipt timed out")); await cancelling;
  assert.equal(run.state().status.phase, "completed"); assert.equal(run.state().unconfirmed, false);
  assert.equal(run.state().cancelRequested, false); assert.equal(run.reads.length, 2);
  assert.equal(run.completions.length, 1);
  run.controller.disconnect();
});

test("read failures keep the native operation busy and later snapshots recover without another write", async () => {
  const run = setup(); run.reads[0].resolve(status("copying")); await flush();
  run.tick(); run.reads[1].reject(new Error("pipe temporarily unavailable")); await flush();
  assert.equal(run.state().status.phase, "copying"); assert.match(run.state().error, /temporarily unavailable/);
  await run.controller.migrate(input); assert.equal(run.calls.length, 0);
  run.tick(); run.reads[2].resolve(status("verifying")); await flush();
  assert.equal(run.state().status.phase, "verifying"); assert.equal(run.state().error, null);
  run.controller.disconnect();
});


test("an automatic selection executes only its acknowledged matching ready operation", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const selecting = run.controller.migrate(input);
  run.calls[0].resolve(status("checking")); await selecting;
  assert.equal(run.calls.length, 1, "Checking cannot move files");
  run.reads[1].resolve(status()); await flush();
  assert.equal(run.calls.length, 2);
  assert.equal(run.calls[1].command, "execute");
  assert.equal(run.calls[1].argument, "migration-one");
  run.calls[1].resolve(status("copying")); await flush();
  run.reads[2].resolve(status("copying")); await flush();
  assert.equal(run.calls.length, 2, "Observing progress cannot execute again");
  run.controller.disconnect();
});


test("old ready results and reconnects cannot execute without a new selection", async () => {
  const run = setup(); run.reads[0].resolve(status()); await flush();
  assert.equal(run.calls.length, 0);
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().draft)), input);
  const request = run.controller.migrate(input);
  run.calls[0].resolve(status("checking", { operation_id: "second" })); await request;
  run.controller.disconnect(); run.reads[1].resolve(status("ready", { operation_id: "second" })); await flush();
  run.controller.connect(); run.reads[2].resolve(status("ready", { operation_id: "second" })); await flush();
  assert.equal(run.calls.length, 1, "Reload must not revive automatic execution intent");
  run.controller.disconnect();
});

test("a different identity or changed targets cannot satisfy the current selection", async () => {
  for (const change of [{ operation_id: "unrelated" }, { input: { ...input, games_root: "D:/different" } }]) {
    const run = setup(); run.reads[0].resolve(null); await flush();
    const request = run.controller.migrate(input); run.calls[0].resolve(status("checking")); await request;
    run.reads[1].resolve(status("ready", change)); await flush();
    assert.equal(run.state().unconfirmed, true); assert.equal(run.calls.length, 1);
    run.controller.disconnect();
  }
});

test("a stale preview receipt cannot reuse a previous ready operation", async () => {
  const run = setup(); run.reads[0].resolve(status()); await flush();
  const request = run.controller.migrate(input); run.calls[0].resolve(status()); await request;
  assert.equal(run.state().unconfirmed, true); assert.equal(run.calls.length, 1);
  run.reads[1].resolve(status()); await flush();
  assert.equal(run.calls.length, 1);
  run.controller.disconnect();
});

test("a lost preview receipt queries state but never automatically executes it", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const request = run.controller.migrate(input); run.calls[0].reject(new Error("preview receipt lost")); await request;
  assert.equal(run.state().unconfirmed, true);
  run.reads[1].resolve(status("checking")); await flush();
  run.tick(); run.reads[2].resolve(status()); await flush();
  assert.equal(run.state().unconfirmed, false); assert.equal(run.calls.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().draft)), input);
  run.controller.disconnect();
});

test("a previous completion cannot clear a new draft after its preview receipt is lost", async () => {
  const run = setup(); run.reads[0].resolve(status("completed")); await flush();
  const selected = { ...input, games_root: "F:/games" };
  const request = run.controller.migrate(selected); run.calls[0].reject(new Error("preview failed")); await request;
  run.reads[1].resolve(status("completed")); await flush();
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().draft)), selected);
  assert.equal(run.completions.length, 1); assert.equal(run.calls.length, 1);
  assert.equal(run.state().unconfirmed, false);
  assert.equal(run.state().status, null, "An older completed status must not label the new rejected selection as successful");
  assert.match(run.state().error, /preview failed/);
  run.controller.disconnect();
});

test("cancelling the current scan revokes automatic intent before a late ready result", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const request = run.controller.migrate(input); run.calls[0].resolve(status("checking")); await request;
  const cancelling = run.controller.cancel();
  run.reads[1].resolve(status()); await flush();
  assert.equal(run.calls.length, 2); assert.equal(run.calls[1].command, "cancel");
  run.calls[1].resolve(status()); await cancelling;
  assert.equal(run.state().cancelRequested, true);
  run.reads[2].resolve(status()); await flush(); assert.equal(run.calls.length, 2);
  run.tick(); run.reads[3].resolve(status("cancelled")); await flush();
  assert.equal(run.state().cancelRequested, false);
  run.controller.disconnect();
});

test("a conflict-free later poll cannot bypass the conflict that stopped automatic migration", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const request = run.controller.migrate(input); run.calls[0].resolve(status("checking")); await request;
  run.reads[1].resolve(status("ready", { conflict_count: 1, conflicts: [{ source: "E:/one", target: "D:/one", message: "Unsafe target" }] })); await flush();
  run.controller.recheck(); run.reads[2].resolve(status()); await flush();
  assert.equal(run.calls.length, 1);
  const retry = run.controller.migrate(input); run.calls[1].resolve(status("ready", { operation_id: "retry" })); await retry;
  assert.equal(run.calls[2].command, "execute"); assert.equal(run.calls[2].argument, "retry");
  run.calls[2].resolve(status("copying", { operation_id: "retry" })); await flush();
  run.controller.disconnect();
});

test("a failed draft can be discarded without another native write", async () => {
  const run = setup(); run.reads[0].resolve(null); await flush();
  const request = run.controller.migrate(input); run.calls[0].resolve(status("failed", { error: "Target invalid" })); await request;
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().draft)), input);
  await run.controller.discard(); assert.equal(run.state().draft, null); assert.equal(run.calls.length, 1);
  assert.equal(run.state().hasActivity, false, "Discarding a confirmed failed selection must remove its activity");
  run.controller.disconnect();
});


test("terminal outcomes and idle previews can be dismissed without changing the native outcome", async () => {
  for (const phase of ["failed", "cancelled", "completed", "ready"]) {
    const storage = dismissalStorage();
    const outcome = status(phase, { error: phase === "failed" ? "Target invalid" : null,
      cleanup_warnings: phase === "completed" ? ["Old file retained"] : [],
      conflict_count: phase === "ready" ? 1 : 0 });
    const run = setup(storage); run.reads[0].resolve(outcome); await flush();
    assert.equal(run.state().hasActivity, true); assert.equal(run.state().canDismiss, true);
    run.controller.dismiss();
    assert.equal(run.state().hasActivity, false); assert.equal(run.state().canDismiss, false);
    assert.equal(run.state().status.phase, phase); assert.equal(run.state().draft, null);
    assert.equal(run.calls.length, 0, "Closing a notification cannot send native file commands");
    run.controller.recheck(); run.reads[1].resolve(outcome); await flush();
    assert.equal(run.state().hasActivity, false, "Reading the same native result must not reopen it");
    run.controller.disconnect(); run.controller.connect(); run.reads[2].resolve(outcome); await flush();
    assert.equal(run.state().hasActivity, false); assert.equal(run.state().draft, null);
    run.controller.disconnect();
    const reopened = setup(storage); reopened.reads[0].resolve(outcome); await flush();
    assert.equal(reopened.state().hasActivity, false); assert.equal(reopened.state().draft, null);
    const next = reopened.controller.migrate({ ...input, games_root: "F:/games" });
    reopened.calls[0].resolve(status("failed", { operation_id: "new-operation", error: "New failure" })); await next;
    assert.equal(reopened.state().hasActivity, true, "A new user selection must report its own result");
    reopened.controller.disconnect();
  }
});

test("a confirmed preview error without a native operation can be closed or discarded", async () => {
  for (const action of ["dismiss", "discard"]) {
    const run = setup(); run.reads[0].resolve(null); await flush();
    const pending = run.controller.migrate(input); run.calls[0].reject(new Error("Preview rejected")); await pending;
    assert.equal(run.state().canDismiss, false, "A missing receipt is still an unknown outcome");
    run.reads[1].resolve(null); await flush();
    assert.equal(run.state().status, null); assert.equal(run.state().unconfirmed, false);
    assert.equal(run.state().hasActivity, true); assert.equal(run.state().canDismiss, true);
    await run.controller[action]();
    assert.equal(run.state().hasActivity, false); assert.equal(run.state().error, null);
    assert.equal(run.state().draft, null); assert.equal(run.calls.length, 1);
    run.controller.disconnect();
  }
});

test("activity, unknown outcomes and recovery barriers cannot be dismissed", async () => {
  for (const phase of ["checking", "copying", "verifying", "applying", "cleaning", "failed", "completed"]) {
    const storage = dismissalStorage();
    const run = setup(storage);
    run.controller.dismiss(); assert.equal(run.state().recovering, true);
    run.reads[0].resolve(status(phase, { recovery_required: ["failed", "completed"].includes(phase) })); await flush();
    assert.equal(run.state().hasActivity, true); assert.equal(run.state().canDismiss, false);
    run.controller.dismiss(); await run.controller.discard();
    assert.equal(run.state().hasActivity, true); assert.equal(storage.values.size, 0);
    assert.equal(run.calls.length, 0); run.controller.disconnect();
  }
  const run = setup(); run.reads[0].resolve(null); await flush();
  const pending = run.controller.migrate(input); run.calls[0].reject(new Error("Receipt lost")); await pending;
  run.controller.dismiss(); await run.controller.discard();
  assert.equal(run.state().unconfirmed, true); assert.equal(run.state().hasActivity, true);
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().draft)), input);
  run.controller.disconnect();
});

test("a recovered safety barrier overrides a prior dismissal for the same operation", async () => {
  const storage = dismissalStorage();
  const run = setup(storage); run.reads[0].resolve(status("failed")); await flush();
  run.controller.dismiss(); assert.equal(run.state().hasActivity, false); run.controller.disconnect();
  const recovered = setup(storage); recovered.reads[0].resolve(status("failed", { recovery_required: true })); await flush();
  assert.equal(recovered.state().hasActivity, true); assert.equal(recovered.state().canDismiss, false);
  recovered.controller.disconnect();
});

test("dismissal persistence failure keeps the result visible and can be retried", async () => {
  const storage = dismissalStorage(); const write = storage.setItem; let fail = true;
  storage.setItem = (key, value) => { if (fail) throw new Error("Preference storage unavailable"); write(key, value); };
  const run = setup(storage); run.reads[0].resolve(status("failed")); await flush();
  run.controller.dismiss();
  assert.equal(run.state().hasActivity, true); assert.equal(run.state().canDismiss, true);
  assert.match(run.state().dismissError, /Preference storage unavailable/);
  assert.equal(run.state().status.phase, "failed"); assert.equal(storage.values.size, 0);
  fail = false; run.controller.dismiss();
  assert.equal(run.state().hasActivity, false); assert.equal(run.state().dismissError, null);
  assert.equal(run.calls.length, 0); run.controller.disconnect();
});

test("closing a refresh failure preserves the committed-path evidence and explicit refresh can reopen feedback", async () => {
  const run = setup(); run.callbacks.onCompleted = async () => { throw new Error("Refresh failed"); };
  run.reads[0].resolve(status("completed")); await flush();
  const refreshError = run.state().refreshError; assert.match(refreshError, /Refresh failed/);
  run.controller.dismiss();
  assert.equal(run.state().hasActivity, false); assert.equal(run.state().refreshError, refreshError);
  assert.deepEqual(JSON.parse(JSON.stringify(run.state().status.input)), input);
  await run.controller.refresh();
  assert.equal(run.state().hasActivity, true); assert.equal(run.state().refreshError, refreshError);
  run.controller.dismiss(); run.controller.disconnect();
});

test("unreadable or invalid dismissal preferences keep outcomes visible until acknowledgement can be saved", async () => {
  for (const read of [() => { throw new Error("Storage blocked"); }, () => "not json", () => JSON.stringify([null])]) {
    const storage = dismissalStorage(); storage.getItem = read;
    const run = setup(storage); run.reads[0].resolve(status("failed")); await flush();
    assert.equal(run.state().hasActivity, true); assert.ok(run.state().dismissError);
    run.controller.dismiss();
    assert.equal(run.state().hasActivity, false); assert.equal(run.state().dismissError, null);
    assert.equal(storage.values.size, 1); run.controller.disconnect();
  }
});

test("dismissal history remains bounded and keeps the most recent operation identities", async () => {
  const storage = dismissalStorage();
  for (let index = 0; index < 35; index++) {
    const run = setup(storage); run.reads[0].resolve(status("failed", { operation_id: `operation-${index}` })); await flush();
    run.controller.dismiss(); run.controller.disconnect();
  }
  assert.equal(storage.values.size, 1);
  const remembered = JSON.parse([...storage.values.values()][0]);
  assert.equal(remembered.length, 32); assert.equal(remembered[0], "operation-3"); assert.equal(remembered[31], "operation-34");
});
