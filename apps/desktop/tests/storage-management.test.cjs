const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { randomUUID } = require("node:crypto");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function load(file, dependencies = {}) {
  const source = fs.readFileSync(path.join(__dirname, "../src", file), "utf8");
  const module = { exports: {} };
  vm.runInNewContext(transpileTypeScript(source, file), { module, exports: module.exports,
    require: (name) => {
      if (name === "./storage-management-requests" || name === "./i18n-config") return load(`${name.slice(2)}.ts`, dependencies);
      if (name === "./locale-preference") return { readPreferredLocale: () => "en-US" };
      assert.ok(name in dependencies, `Unexpected import ${name}`); return dependencies[name];
    },
    crypto: { randomUUID }, structuredClone, TextEncoder, setTimeout });
  return module.exports;
}

test("storage IPC carries opaque IDs and never falls back to LAN", async () => {
  const calls = [];
  let tauri = true;
  let lan = true;
  const api = load("api-storage.ts", {
    "@tauri-apps/api/core": { isTauri: () => tauri },
    "./api-transport": { shouldUseLanApi: () => lan, invokeOrMock: async (command, args) => { calls.push({ command, args }); } }
  });
  await api.listInstanceArchives();
  await api.readInstanceArchiveDetails("archive-preview");
  await api.restoreInstanceArchive("archive-one");
  await api.purgeInstanceArchive("archive-two");
  await api.scanStorageUsage("scan-one");
  await api.cancelStorageUsageScan("scan-one");
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { command: "list_instance_archives" },
    { command: "read_instance_archive_details", args: { input: { archive_id: "archive-preview" } } },
    { command: "restore_instance_archive", args: { input: { archive_id: "archive-one" } } },
    { command: "purge_instance_archive", args: { input: { archive_id: "archive-two" } } },
    { command: "scan_storage_usage", args: { input: { scan_id: "scan-one" } } },
    { command: "cancel_storage_usage_scan", args: { input: { scan_id: "scan-one" } } }
  ]);
  tauri = false;
  assert.equal(api.isStorageManagementAvailable(), false);
  for (const call of [api.listInstanceArchives, () => api.restoreInstanceArchive("a"), () => api.purgeInstanceArchive("a"),
    () => api.readInstanceArchiveDetails("a"),
    () => api.scanStorageUsage("s"), () => api.cancelStorageUsageScan("s")]) await assert.rejects(call, /desktop host/);
  assert.equal(calls.length, 6);
  lan = false;
  assert.equal(api.isStorageManagementAvailable(), true, "Isolated development preview remains usable");
});

test("program inspection sends owned identifiers and remains available through authenticated LAN transport", async () => {
  const calls = [];
  const inventory = { requires_archive_inventory: false, installations: [{
    id: 1, install_root: "D:/games/minecraft", scope: "library", install_state: "NotInstalled",
    pending_removal: true, current_version: null, used_by: [], modification_state: "unverified", size_bytes: 0
  }], creation: {
    can_create: true, action: "independent_install", program_path: "D:/instances/minecraft/runtime",
    additional_bytes: 0, reason: null
  } };
  const removal = { program_path: "D:/instances/instance-one/runtime", data_path: "D:/instances/instance-one",
    remove_program: true, preserved_program_path: null, owned_data_paths: ["D:/instances/instance-one"],
    preserved_external_saves_path: null };
  const api = load("api-storage.ts", {
    "@tauri-apps/api/core": { isTauri: () => false },
    "./api-transport": { shouldUseLanApi: () => true, invokeOrMock: async (command, args) => {
      calls.push({ command, args });
      return command === "inspect_module_programs" ? inventory : removal;
    } }
  });
  assert.strictEqual(await api.inspectModulePrograms("minecraft", "independent", "verified"), inventory);
  assert.equal(inventory.installations[0].pending_removal, true, "Pending journal cleanup survives the inventory IPC boundary");
  assert.strictEqual(await api.inspectInstanceRemoval("instance-one"), removal);
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { command: "inspect_module_programs", args: { input: { module_id: "minecraft", program_mode: "independent", program_source: "verified", include_archived_sources: false } } },
    { command: "inspect_instance_removal", args: { input: { instance_id: "instance-one" } } }
  ]);
});

function archived(model, id = "instance-one") {
  const details = { summary: { id, name: "Archived world" }, settings_json: '{"seed":"preserve"}', config_file_path: `D:/instances/${id}/settings.json` };
  const backup = { backup_id: "retained-backup", total_bytes: 23 };
  const program = { mode: "independent", root: `D:/instances/${id}/runtime` };
  model.archive({ archive_id: `archive-${id}`, external_saves_backup_id: null, instance_id: id, instance_name: details.summary.name, module_id: "minecraft", deleted_at_unix_ms: 123,
    archived_instance_root: `D:/instance-archives/${id}-123`, previous_instance_root: `D:/instances/${id}` }, details, [backup], program);
  return { details, backup, program, id: model.list("D:/instance-archives").archives.find((entry) => entry.instance_id === id).archive_id };
}

test("development archive preserves data and rejects conflicting or out-of-scope operations", () => {
  const { MockStorageManagement } = load("api-mock/storage-management.ts");
  const model = new MockStorageManagement();
  const fixture = archived(model);
  fixture.details.settings_json = "changed after archive";
  assert.equal(model.list("D:/another-root").archives.length, 0);
  assert.equal(model.list("D:/instances").archives.length, 0, "Archives follow their configured root instead of the active workspace");
  assert.throws(() => model.restore(fixture.id, "D:/another-root", []), /no longer exists/);
  assert.throws(() => model.restore(fixture.id, "D:/instance-archives", ["instance-one"]), /already owns/);
  assert.equal(model.list("D:/instance-archives").archives.length, 1);
  const restored = model.restore(fixture.id, "D:/instance-archives", []);
  assert.equal(restored.details.settings_json, '{"seed":"preserve"}');
  assert.deepEqual(restored.backups, [fixture.backup]);
  assert.deepEqual(restored.program, fixture.program);
  assert.equal(model.list("D:/instance-archives").archives.length, 0);
  const purge = archived(model, "instance-two");
  assert.equal(model.purge(purge.id, "D:/instance-archives").purged, true);
  assert.throws(() => model.restore(purge.id, "D:/instance-archives", []), /no longer exists/);
});

test("development archive details preserve saved configuration, policies and backups without restoring or editing", () => {
  const { MockStorageManagement } = load("api-mock/storage-management.ts");
  const model = new MockStorageManagement();
  const details = { summary: { id: "saved-world", name: "Retained name", module_id: "minecraft", bind_ip: "127.0.0.2", autostart: true },
    auto_backup_on_stop: false, backup_retention_count: 7,
    config_file_path: "D:/instances/saved-world/config/instance.json", active_run: { run_id: 9, pid: 111 },
    settings_json: '{"motd":"Saved value","max_players":7,"unknown":{"enabled":false}}',
    ports: [{ name: "game", protocol: "tcp", port: 27981 }] };
  const backup = { backup_id: "world-backup", instance_id: details.summary.id, display_name: "Retained backup",
    backup_kind: "manual", created_at_unix_ms: 123, file_count: 3, total_bytes: 23,
    backup_path: "D:/instances/saved-world/backups/world-backup", saves_path: "D:/instances/saved-world/saves" };
  model.archive({ archive_id: "saved-archive", external_saves_backup_id: null, instance_id: details.summary.id,
    instance_name: details.summary.name, module_id: "minecraft", deleted_at_unix_ms: 123,
    archived_instance_root: "D:/instance-archives/saved-archive", previous_instance_root: "D:/instances/saved-world" },
    details, [backup], { mode: "independent", root: "D:/instances/saved-world/runtime" });
  const first = model.details("saved-archive", "D:/instance-archives");
  assert.equal(first.instance.settings_json, details.settings_json);
  assert.equal(first.instance.summary.bind_ip, "127.0.0.2");
  assert.deepEqual(first.instance.ports, details.ports);
  assert.equal(first.instance.summary.id, details.summary.id);
  assert.equal(first.instance.summary.name, details.summary.name);
  assert.equal(first.instance.active_run, null);
  assert.equal(first.instance.summary.active_process_count, 0);
  assert.equal(first.instance.config_file_path, "D:/instance-archives/saved-archive/config/instance.json");
  assert.deepEqual(first.maintenance, { autostart: true, auto_backup_on_stop: false,
    backup_retention_count: 7, crash_restart_limit: null, runtime_mode: "independent" });
  assert.deepEqual(first.runs, { entries: [], total: 0, truncated: false });
  assert.ok(first.log.issues.some((issue) => issue.includes("does not retain runtime logs")));
  assert.equal(first.backups.entries[0].display_name, backup.display_name);
  assert.equal(first.backups.entries[0].backup_path, "D:/instance-archives/saved-archive/backups/world-backup");
  first.instance.ports[0].port = 1;
  first.maintenance.backup_retention_count = 1;
  first.backups.entries[0].display_name = "Changed preview";
  first.instance.settings_json = "{}";
  first.instance.summary.name = "Changed preview name";
  const second = model.details("saved-archive", "D:/instance-archives");
  assert.equal(second.instance.settings_json, details.settings_json);
  assert.equal(second.instance.ports[0].port, 27981);
  assert.equal(second.instance.summary.name, "Retained name");
  assert.equal(second.maintenance.backup_retention_count, 7);
  assert.equal(second.backups.entries[0].display_name, backup.display_name);
  assert.equal(model.list("D:/instance-archives").archives.length, 1);
  assert.throws(() => model.details("saved-archive", "D:/other-root"), /no longer exists/);
  const restored = model.restore("saved-archive", "D:/instance-archives", []);
  assert.deepEqual(restored.details, details);
  assert.deepEqual(restored.backups, [backup]);
  assert.throws(() => model.details("saved-archive", "D:/instance-archives"), /no longer exists/);
});

test("development preview cannot hide retained archives by changing their roots", () => {
  const { MockStorageManagement } = load("api-mock/storage-management.ts");
  const model = new MockStorageManagement();
  const settings = { servers_root: "D:/instances", archives_root: "D:/instance-archives" };
  const next = { ...settings, archives_root: "D:/other-archives" };
  assert.doesNotThrow(() => model.validatePathChange(settings, next));
  const fixture = archived(model);
  assert.throws(() => model.validatePathChange(settings, next), /Restore or permanently delete/);
  assert.throws(() => model.validatePathChange(settings, { ...settings, servers_root: "D:/other-instances" }), /Restore or permanently delete/);
  assert.doesNotThrow(() => model.validatePathChange(settings, { ...settings, archives_root: "d:\\INSTANCE-ARCHIVES\\" }));
  model.purge(fixture.id, settings.archives_root);
  assert.doesNotThrow(() => model.validatePathChange(settings, next));
});

test("development scan reports unknown physical allocation and retains cancellation ownership", async () => {
  const { MockStorageManagement } = load("api-mock/storage-management.ts");
  const model = new MockStorageManagement();
  const fixture = archived(model);
  const settings = { servers_root: "D:/instances", archives_root: "D:/instance-archives" };
  const pending = model.scan("first", settings, [fixture.details]);
  await assert.rejects(() => model.scan("second", settings, []), /already running/);
  assert.equal(model.cancel("unknown"), false);
  assert.equal(model.cancel("first"), true);
  const cancelled = await pending;
  assert.equal(cancelled.status, "cancelled");
  assert.equal(cancelled.allocated_bytes, null);
  assert.equal(model.cancel("first"), false);
  const report = await model.scan("third", settings, [fixture.details]);
  assert.equal(report.status, "partial");
  assert.equal(report.allocated_bytes, null);
  assert.equal(report.entries.length, 2);
  assert.ok(report.issues.some((issue) => issue.includes("does not measure disk usage")));
});
