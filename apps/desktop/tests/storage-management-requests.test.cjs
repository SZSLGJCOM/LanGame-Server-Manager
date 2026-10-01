const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const settle = () => new Promise((resolve) => setImmediate(resolve));
const inventory = (id) => ({ archives: [{ archive_id: id }], pending_deletions: [], issues: [] });

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness() {
  const calls = [], modules = new Map();
  let native = true, lan = false, synchronousFailure = null;
  const invokeOrMock = (command, args) => {
    const completion = deferred();
    calls.push({ command, args, ...completion });
    if (synchronousFailure) {
      const error = synchronousFailure; synchronousFailure = null; throw error;
    }
    return completion.promise;
  };
  const dependencies = {
    "@tauri-apps/api/core": { isTauri: () => native },
    "./api-transport": { shouldUseLanApi: () => lan, invokeOrMock },
    "./locale-preference": { readPreferredLocale: () => "en-US" }
  };
  function load(file) {
    if (modules.has(file)) return modules.get(file).exports;
    const filename = path.join(__dirname, "../src", file);
    const module = { exports: {} }; modules.set(file, module);
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
      module, exports: module.exports,
      require(name) {
        if (name in dependencies) return dependencies[name];
        assert.ok(["./storage-management-requests", "./i18n-config"].includes(name), `Unexpected dependency ${name}`);
        return load(`${name.slice(2)}.ts`);
      }
    }, { filename });
    return module.exports;
  }
  return { storage: load("api-storage.ts"), api: load("api.ts"), calls,
    commands: () => calls.map((call) => call.command),
    failSynchronously: (error) => { synchronousFailure = error; },
    useLan: () => { native = false; lan = true; } };
}

test("archive reads share one in-flight promise and do not cache either results or failures", async () => {
  const h = harness();
  const reads = Array.from({ length: 16 }, () => h.storage.listInstanceArchives());
  assert.ok(reads.every((read) => read === reads[0]));
  await settle(); assert.deepEqual(h.commands(), ["list_instance_archives"]);
  const first = inventory("before"); h.calls[0].resolve(first);
  assert.ok((await Promise.all(reads)).every((value) => value === first));
  const next = h.storage.listInstanceArchives(); assert.notEqual(next, reads[0]);
  const failure = new Error("inventory inaccessible");
  const rejection = assert.rejects(next, (error) => error === failure);
  await settle(); assert.equal(h.calls.length, 2); h.calls[1].reject(failure); await rejection;
  await settle(); assert.equal(h.calls.length, 2, "A real read failure is never retried automatically");
  const retry = h.storage.listInstanceArchives(); await settle(); assert.equal(h.calls.length, 3);
  h.calls[2].resolve(inventory("retry")); assert.equal((await retry).archives[0].archive_id, "retry");
});

for (const readFails of [false, true]) {
  test(`an admitted mutation waits for the preceding read (${readFails ? "failure" : "success"}) and later reads wait for fresh data`, async () => {
    const h = harness(), readFailure = new Error("preceding read failure");
    const before = h.storage.listInstanceArchives();
    const observedBefore = readFails ? assert.rejects(before, (error) => error === readFailure) : before;
    const mutation = h.storage.restoreInstanceArchive("archive-one");
    const after = h.storage.listInstanceArchives();
    assert.notEqual(after, before, "A post-mutation reader must not observe the preceding inventory");
    assert.equal(h.storage.listInstanceArchives(), after);
    await assert.rejects(h.storage.purgeInstanceArchive("duplicate"), /storage operation is in progress/);
    await settle(); assert.deepEqual(h.commands(), ["list_instance_archives"]);
    if (readFails) h.calls[0].reject(readFailure); else h.calls[0].resolve(inventory("before"));
    await observedBefore; await settle();
    assert.deepEqual(h.commands(), ["list_instance_archives", "restore_instance_archive"]);
    assert.equal(h.calls[1].args.input.archive_id, "archive-one");
    h.calls[1].resolve({ restored: true }); await mutation; await settle();
    assert.deepEqual(h.commands(), ["list_instance_archives", "restore_instance_archive", "list_instance_archives"]);
    h.calls[2].resolve(inventory("after")); assert.equal((await after).archives[0].archive_id, "after");
  });
}

test("a failed mutation retains its original error and releases one queued read without retrying the write", async () => {
  const h = harness(), failure = new Error("permission denied");
  const mutation = h.storage.purgeInstanceArchive("archive-one");
  const rejected = assert.rejects(mutation, (error) => error === failure);
  const reads = Array.from({ length: 12 }, () => h.storage.listInstanceArchives());
  assert.ok(reads.every((read) => read === reads[0]));
  await settle(); assert.deepEqual(h.commands(), ["purge_instance_archive"]);
  h.calls[0].reject(failure); await rejected; await settle();
  assert.deepEqual(h.commands(), ["purge_instance_archive", "list_instance_archives"]);
  h.calls[1].resolve(inventory("partial-purge")); await Promise.all(reads);
  await settle(); assert.equal(h.calls.length, 2);
  const explicitRetry = h.storage.purgeInstanceArchive("archive-one"); await settle();
  assert.equal(h.calls[2].command, "purge_instance_archive"); h.calls[2].resolve({ purged: true }); await explicitRetry;
});

test("archive and deletion APIs share the mutation gate with archive management", async () => {
  const h = harness();
  const archive = h.api.archiveInstance("one");
  await assert.rejects(h.api.deleteInstance("two"), /storage operation is in progress/);
  await assert.rejects(h.storage.restoreInstanceArchive("three"), /storage operation is in progress/);
  await settle(); assert.deepEqual(h.commands(), ["archive_instance_record"]);
  assert.equal(h.calls[0].args.instance_id, "one");
  h.calls[0].resolve({ archived: true }); await archive;
  const deletion = h.api.deleteInstance("two");
  const listing = h.storage.listInstanceArchives();
  await assert.rejects(h.storage.purgeInstanceArchive("three"), /storage operation is in progress/);
  await settle(); assert.deepEqual(h.commands(), ["archive_instance_record", "delete_instance_record"]);
  assert.equal(h.calls[1].args.instanceId, "two");
  h.calls[1].resolve({ deleted: true }); await deletion; await settle();
  assert.equal(h.calls[2].command, "list_instance_archives"); h.calls[2].resolve(inventory("retained")); await listing;
});

test("scan cancellation bypasses the mutation gate but does not release ownership before scan completion", async () => {
  const h = harness();
  const scan = h.storage.scanStorageUsage("scan-one");
  const listing = h.storage.listInstanceArchives();
  await settle(); assert.deepEqual(h.commands(), ["scan_storage_usage"]);
  const cancellation = h.storage.cancelStorageUsageScan("scan-one");
  assert.deepEqual(h.commands(), ["scan_storage_usage", "cancel_storage_usage_scan"]);
  assert.equal(h.calls[1].args.input.scan_id, "scan-one");
  h.calls[1].resolve(true); assert.equal(await cancellation, true);
  await assert.rejects(h.api.archiveInstance("one"), /storage operation is in progress/);
  await settle(); assert.equal(h.calls.length, 2, "Cancellation acknowledgement must not start the waiting read");
  h.calls[0].resolve({ status: "cancelled" }); await scan; await settle();
  assert.equal(h.calls[2].command, "list_instance_archives"); h.calls[2].resolve(inventory("after-scan")); await listing;
});

test("removal previews and cached bootstrap remain independent of archive mutation serialization", async () => {
  const h = harness();
  const mutation = h.storage.restoreInstanceArchive("archive-one"); await settle();
  const bootstrap = h.api.bootstrapApp();
  const removalRead = h.storage.inspectInstanceRemoval("one");
  assert.deepEqual(h.commands(), ["restore_instance_archive", "bootstrap", "inspect_instance_removal"]);
  h.calls[1].resolve({ state: {} }); h.calls[2].resolve({ removable: true }); await Promise.all([bootstrap, removalRead]);
  h.calls[0].resolve({ restored: true }); await mutation;
});

test("a scan is admitted immediately or rejected without becoming a delayed uncancellable request", async () => {
  const h = harness();
  const listing = h.storage.listInstanceArchives();
  await assert.rejects(h.storage.scanStorageUsage("blocked-by-read"), /storage operation is in progress/);
  await settle(); assert.deepEqual(h.commands(), ["list_instance_archives"]);
  h.calls[0].resolve(inventory("ready")); await listing;
  const mutation = h.api.archiveInstance("one");
  await assert.rejects(h.storage.scanStorageUsage("blocked-by-write"), /storage operation is in progress/);
  h.calls[1].resolve({ archived: true }); await mutation; await settle();
  assert.deepEqual(h.commands(), ["list_instance_archives", "archive_instance_record"]);
  const scan = h.storage.scanStorageUsage("admitted");
  assert.equal(h.calls[2].command, "scan_storage_usage", "The scan ID reaches the native bridge before cancellation can be requested");
  const cancellation = h.storage.cancelStorageUsageScan("admitted");
  assert.equal(h.calls[3].command, "cancel_storage_usage_scan");
  h.calls[3].resolve(true); await cancellation; h.calls[2].resolve({ status: "cancelled" }); await scan;
});

test("synchronous transport failures release the mutation gate without swallowing diagnostics", async () => {
  const h = harness(), failure = new Error("bridge unavailable"); h.failSynchronously(failure);
  await assert.rejects(h.api.archiveInstance("one"), (error) => error === failure);
  const next = h.api.deleteInstance("one"); await settle();
  assert.deepEqual(h.commands(), ["archive_instance_record", "delete_instance_record"]);
  h.calls[1].resolve({ deleted: true }); await next;
});

test("host-only requests reject before joining a native request and do not poison the shared gate", async () => {
  const h = harness(); h.useLan();
  for (const request of [() => h.storage.listInstanceArchives(), () => h.storage.restoreInstanceArchive("a"),
    () => h.storage.readInstanceArchiveDetails("a"),
    () => h.storage.purgeInstanceArchive("a"), () => h.storage.scanStorageUsage("s"),
    () => h.storage.cancelStorageUsageScan("s"), () => h.api.archiveInstance("one")]) {
    await assert.rejects(request(), /desktop host/);
  }
  assert.equal(h.calls.length, 0);
  const deletion = h.api.deleteInstance("one"); await settle();
  assert.deepEqual(h.commands(), ["delete_instance_record"], "Existing LAN deletion capability is preserved");
  h.calls[0].resolve({ deleted: true }); await deletion;
});

test("archive configuration inspections serialize with inventory, later selections and mutations", async () => {
  const h = harness();
  const listing = h.storage.listInstanceArchives();
  const first = h.storage.readInstanceArchiveDetails("first");
  const second = h.storage.readInstanceArchiveDetails("second");
  const purge = h.storage.purgeInstanceArchive("second");
  const after = h.storage.listInstanceArchives();
  await settle(); assert.deepEqual(h.commands(), ["list_instance_archives"]);
  h.calls[0].resolve(inventory("before")); await listing; await settle();
  assert.equal(h.calls[1].command, "read_instance_archive_details");
  assert.equal(h.calls[1].args.input.archive_id, "first");
  h.calls[1].resolve({ archive_id: "first", settings_json: '{"motd":"retained"}' });
  assert.equal((await first).settings_json, '{"motd":"retained"}'); await settle();
  assert.equal(h.calls[2].args.input.archive_id, "second");
  h.calls[2].resolve({ archive_id: "second" }); await second; await settle();
  assert.equal(h.calls[3].command, "purge_instance_archive");
  h.calls[3].resolve({ purged: true }); await purge; await settle();
  assert.equal(h.calls[4].command, "list_instance_archives");
  h.calls[4].resolve(inventory("after")); await after;
});

test("obsolete queued archive selections cancel before native dispatch and release inspection ownership", async () => {
  const h = harness(), controller = new AbortController();
  const first = h.storage.readInstanceArchiveDetails("first"); await settle();
  const obsolete = h.storage.readInstanceArchiveDetails("obsolete", { signal: controller.signal });
  const refused = assert.rejects(obsolete, (error) => error.name === "AbortError");
  const latest = h.storage.readInstanceArchiveDetails("latest");
  controller.abort();
  assert.equal(h.calls.length, 1);
  h.calls[0].resolve({ archive_id: "first" }); await first; await refused; await settle();
  assert.deepEqual(h.calls.map((call) => call.args.input.archive_id), ["first", "latest"]);
  h.calls[1].resolve({ archive_id: "latest" }); await latest;
});

test("archive inspections reject on LAN after queuing and release ownership after a native failure", async () => {
  const h = harness();
  const read = h.storage.readInstanceArchiveDetails("first"); await settle();
  const failure = new Error("Archived configuration checksum mismatch");
  const refused = assert.rejects(read, (error) => error === failure);
  const queued = h.storage.readInstanceArchiveDetails("queued");
  const denied = assert.rejects(queued, /desktop host/);
  h.useLan(); h.calls[0].reject(failure); await refused; await denied;
  assert.equal(h.calls.length, 1, "No queued host-only file read may dispatch over LAN");
});

test("archive inspection queues remain bounded and cancellation drains pending selections", async () => {
  const h = harness(), controller = new AbortController();
  const reads = Array.from({ length: 128 }, (_, index) =>
    h.storage.readInstanceArchiveDetails(String(index), { signal: controller.signal }));
  const observed = reads.map((read) => assert.rejects(read, (error) => error.name === "AbortError"));
  const overflow = assert.rejects(h.storage.readInstanceArchiveDetails("overflow"), /Too many pending archive reads/);
  controller.abort(); await overflow; await Promise.all(observed);
  assert.equal(h.calls.length, 0);
});

for (const operation of ["archive", "restore"]) {
  test(`a queued ${operation} rechecks host access before dispatch after transport switches to LAN`, async () => {
    const h = harness();
    const read = h.storage.listInstanceArchives(); await settle();
    const mutation = operation === "archive" ? h.api.archiveInstance("one") : h.storage.restoreInstanceArchive("one");
    const refusal = assert.rejects(mutation, /desktop host/);
    h.useLan(); h.calls[0].resolve(inventory("before")); await read; await refusal;
    assert.deepEqual(h.commands(), ["list_instance_archives"], "The queued host-only command never reaches LAN transport");
    const deletion = h.api.deleteInstance("one"); await settle();
    assert.equal(h.calls[1].command, "delete_instance_record", "A denied queued operation releases its gate without removing LAN deletion support");
    h.calls[1].resolve({ deleted: true }); await deletion;
  });
}

test("a queued inventory refresh also rechecks host access when its preceding mutation completes", async () => {
  const h = harness();
  const mutation = h.storage.restoreInstanceArchive("one");
  const read = h.storage.listInstanceArchives(), refusal = assert.rejects(read, /desktop host/);
  h.useLan(); h.calls[0].resolve({ restored: true }); await mutation; await refusal;
  assert.deepEqual(h.commands(), ["restore_instance_archive"]);
});

test("configuration details can load during inventory refresh without requesting or fabricating program counts", async () => {
  const h = harness();
  const listing = h.storage.listInstanceArchives();
  await settle();
  const configuration = h.api.readModuleDetails("minecraft", { includePreservedProgramCounts: false });
  await settle();
  assert.deepEqual(h.commands(), ["list_instance_archives", "read_module_details"]);
  const request = h.calls[1];
  assert.equal(request.args.moduleId, "minecraft");
  assert.equal(request.args.module_id, "minecraft");
  assert.equal(request.args.includePreservedProgramCounts, false);
  assert.equal(request.args.include_preserved_program_counts, false);
  const nativeDetails = { summary: { id: "minecraft", instance_program_count: 0, archived_program_count: 0 },
    schema_json: '{"properties":{"server_name":{"type":"string"}}}',
    install: { install_root: "synthetic-server" }, process: { executable: "java" },
    default_ports: [{ name: "game", port: 25565 }] };
  request.resolve(nativeDetails);
  const details = await configuration;
  assert.equal(details.schema_json, nativeDetails.schema_json);
  assert.equal(details.install, nativeDetails.install);
  assert.equal(details.process, nativeDetails.process);
  assert.equal(details.default_ports, nativeDetails.default_ports);
  assert.equal(Object.hasOwn(details.summary, "instance_program_count"), false);
  assert.equal(Object.hasOwn(details.summary, "archived_program_count"), false);
  assert.equal(nativeDetails.summary.archived_program_count, 0, "the transport result is not mutated");
  h.calls[0].resolve(inventory("after"));
  await listing;
});

const catalogReads = [
  { name: "library refresh", command: "refresh_modules", read: (h) => h.api.refreshModules(), value: [] },
  { name: "module synchronization", command: "sync_modules_to_storage", read: (h) => h.api.syncModulesToStorage(), value: [] },
  { name: "library details", command: "read_module_details", read: (h) => h.api.readModuleDetails("minecraft"), value: { summary: { id: "minecraft" } } }
];

test("installation status refresh completes during archive mutation without requesting inventory counts", async () => {
  const h = harness();
  const mutation = h.api.archiveInstance("other");
  const read = h.api.refreshModules({ includePreservedProgramCounts: false });
  await settle();
  assert.deepEqual(h.commands(), ["archive_instance_record", "refresh_modules"]);
  assert.equal(h.calls[1].args.includePreservedProgramCounts, false);
  assert.equal(h.calls[1].args.include_preserved_program_counts, false);
  const native = [{ id: "minecraft", install_state: "Installed", instance_program_count: 0, archived_program_count: 0 }];
  h.calls[1].resolve(native);
  const modules = await read;
  assert.equal(modules[0].install_state, "Installed");
  assert.equal(Object.hasOwn(modules[0], "instance_program_count"), false);
  assert.equal(Object.hasOwn(modules[0], "archived_program_count"), false);
  assert.equal(native[0].archived_program_count, 0);
  h.calls[0].resolve({ archived: true }); await mutation;
});

test("existing library inspection completes while an unrelated storage mutation is still running", async () => {
  const h = harness();
  const mutation = h.storage.restoreInstanceArchive("other");
  const inspection = h.storage.inspectModulePrograms("minecraft", "independent", "verified");
  await settle();
  assert.deepEqual(h.commands(), ["restore_instance_archive", "inspect_module_programs"]);
  assert.equal(h.calls[1].args.input.include_archived_sources, false);
  const inventory = { requires_archive_inventory: false, creation: { can_create: true } };
  h.calls[1].resolve(inventory);
  assert.equal(await inspection, inventory);
  h.calls[0].resolve({ restored: true }); await mutation;
});

test("archive-dependent inspection waits for mutation completion and retains archive ownership through native completion", async () => {
  const h = harness();
  const mutation = h.api.archiveInstance("other");
  const inspection = h.storage.inspectModulePrograms("minecraft", "independent", "verified");
  h.calls[1].resolve({ requires_archive_inventory: true, creation: { can_create: false } });
  await settle();
  assert.deepEqual(h.commands(), ["archive_instance_record", "inspect_module_programs"]);
  h.calls[0].resolve({ archived: true }); await mutation; await settle();
  assert.equal(h.calls[2].args.input.include_archived_sources, true);
  const listing = h.storage.listInstanceArchives(); await settle();
  assert.equal(h.calls.length, 3);
  const inventory = { requires_archive_inventory: false, creation: { can_create: true } };
  h.calls[2].resolve(inventory); assert.equal(await inspection, inventory); await settle();
  assert.equal(h.calls[3].command, "list_instance_archives");
  h.calls[3].resolve({ archives: [] }); await listing;
});

for (const { name, command, read, value } of catalogReads) {
  test(`${name} waits for post-deletion inventory before dispatching its preserved-program read`, async () => {
    const h = harness();
    const deletion = h.api.deleteInstance("one");
    const listing = h.storage.listInstanceArchives();
    const catalog = read(h);
    await settle();
    assert.deepEqual(h.commands(), ["delete_instance_record"], "No catalog read may race deletion's inventory lock");
    h.calls[0].resolve({ deleted: true }); await deletion; await settle();
    assert.deepEqual(h.commands(), ["delete_instance_record", "list_instance_archives"]);
    h.calls[1].resolve(inventory("after")); await listing; await settle();
    assert.equal(h.calls[2].command, command);
    h.calls[2].resolve(value);
    assert.equal(await catalog, value);
  });

  test(`${name} retains ownership until native completion so archive refresh cannot overlap it`, async () => {
    const h = harness();
    const catalog = read(h);
    const listing = h.storage.listInstanceArchives();
    await settle(); assert.deepEqual(h.commands(), [command]);
    const failure = new Error("Catalog database read failed");
    const rejected = assert.rejects(catalog, error => Object.is(error, failure));
    h.calls[0].reject(failure); await rejected; await settle();
    assert.equal(h.calls[1].command, "list_instance_archives", "Real failures release the slot and are not swallowed or retried");
    h.calls[1].resolve(inventory("after")); await listing;
  });
}

test("library detail reads continue to request and retain preserved program counts by default", async () => {
  const h = harness();
  const read = h.api.readModuleDetails("minecraft");
  await settle();
  assert.equal(h.calls[0].args.includePreservedProgramCounts, true);
  assert.equal(h.calls[0].args.include_preserved_program_counts, true);
  const details = { summary: { id: "minecraft", instance_program_count: 2, archived_program_count: 3 } };
  h.calls[0].resolve(details);
  assert.equal(await read, details);
});
