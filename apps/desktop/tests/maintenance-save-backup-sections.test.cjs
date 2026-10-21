const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (loaded, filename) => {
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = () => {};
const t = (key) => key;

function load(relative, mocks = {}) {
  const filename = path.join(__dirname, "../src", relative);
  const originalRequire = Module.createRequire(filename);
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  loaded.require = (specifier) => Object.hasOwn(mocks, specifier) ? mocks[specifier] : originalRequire(specifier);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return loaded.exports;
}

function findElement(value, predicate) {
  if (Array.isArray(value)) return value.flatMap((child) => findElement(child, predicate));
  if (!value || typeof value !== "object" || !value.props) return [];
  return [...(predicate(value) ? [value] : []), ...findElement([value.props.children, value.props.policies], predicate)];
}

const children = ["AiBroadcastWorkbench", "ArkClusterPanel", "ArkClusterSnapshotSection", "BackupTable", "DailyMaintenanceEditor",
  "ImmediateWorldSave", "InstanceAutostartEditor", "InstanceIsolationPanel", "InstancePanelReadStatus", "InstanceProgramMaintenance",
  "MaintenanceWorkspace", "MoriaNativeSettingsEditor", "ProgramUpdatePolicyEditor", "RuntimePerformanceEditor", "RuntimeRecoveryEditor",
  "SaveImportPanel", "SaveSlotTable", "SavePolicyEditor", "ScumWipeSettingsEditor"];

function maintenance(moduleId, overrides = {}) {
  const mocks = { "../../api": {}, "../../ark-clusters": { isArkModule: (id) => id === "arksurvivalascended" } };
  for (const name of children) {
    const component = () => null;
    Object.defineProperty(component, "name", { value: name });
    mocks[`./${name}`] = { [name]: component };
  }
  const { ServerMaintenanceWorkspace } = load("views/servers/ServerMaintenanceWorkspace.tsx", mocks);
  const root = ServerMaintenanceWorkspace({ active: true, locale: "zh-CN", t,
    details: { summary: { id: "fixture", module_id: moduleId, status: "stopped", active_process_count: 0 },
      config_file_path: "D:/fixture/config/instance.json", backup_uses_declared_saves_path: true },
    moduleDetails: null, selectedBackups: [], onOpenLocalPath: () => {}, onResumeAutoRefresh: () => {}, ...overrides });
  const sections = root.props.sections;
  const section = (id) => sections.find((entry) => entry.id === id);
  const named = (id, name) => findElement([section(id).persistentContent, section(id).content], (element) => element.type?.name === name);
  return { sections, section, named };
}

test("saves and backups are two consolidated maintenance entries", () => {
  const view = maintenance("minecraft");
  assert.deepEqual(view.sections.map((entry) => entry.id), ["saves", "backups", "runtime"]);
  assert.equal(view.section("saves").title, "servers.maintenance.savesTitle");
  assert.equal(view.section("backups").title, "servers.maintenance.backupsTitle");

  assert.equal(view.named("saves", "ImmediateWorldSave").length, 1);
  assert.deepEqual(view.named("saves", "SavePolicyEditor").map((element) => element.props.scope), ["native"]);
  assert.equal(view.named("saves", "SaveImportPanel").length, 1);
  const [browser] = view.named("saves", "SaveSlotTable");
  assert.equal(browser.props.instanceId, "fixture");
  assert.equal(browser.props.backupId, undefined);
  assert.equal(typeof browser.props.onOpenLocalPath, "function");

  assert.equal(view.named("backups", "BackupTable").length, 1);
  assert.equal(typeof view.named("backups", "BackupTable")[0].props.onOpenLocalPath, "function");
  assert.deepEqual(view.named("backups", "SavePolicyEditor").map((element) => element.props.scope), ["backup"]);
  assert.deepEqual(view.named("backups", "DailyMaintenanceEditor").map((element) => element.props.action), ["backup"]);
  assert.deepEqual(view.named("runtime", "DailyMaintenanceEditor").map((element) => element.props.action), ["restart"]);
  assert.equal(view.named("backups", "ArkClusterSnapshotSection").length, 0, "Only ARK exposes cluster snapshots");
});

test("ARK cluster snapshots live under backups while cluster operations keep their entry", () => {
  const view = maintenance("arksurvivalascended");
  assert.deepEqual(view.sections.map((entry) => entry.id), ["saves", "backups", "runtime", "cluster"]);
  assert.equal(view.section("cluster").title, "servers.maintenance.cluster");
  assert.equal(view.named("backups", "ArkClusterSnapshotSection").length, 1);
  assert.equal(view.named("cluster", "ArkClusterPanel").length, 1);
  assert.equal(view.named("cluster", "ArkClusterSnapshotSection").length, 0);
  assert.equal(view.named("cluster", "ArkClusterPanel")[0].props.renderMaintenance, undefined);
});

test("archived instances keep both entries read-only and never list live files", () => {
  const view = maintenance("arksurvivalascended", {
    archive: { backups: { entries: [], truncated: false, issues: [] }, maintenance: { crash_restart_limit: 3 } }
  });
  assert.equal(view.named("saves", "SaveSlotTable").length, 0);
  assert.equal(view.named("backups", "ArkClusterSnapshotSection").length, 0);
  assert.ok(view.named("saves", "SavePolicyEditor").every((element) => element.props.readOnly === true));
});

test("backup restore is enabled only for writable stopped instances with a restore handler", () => {
  const { BackupTable } = load("views/servers/BackupTable.tsx", {
    "./SaveFileBrowser": { SaveFileBrowser: () => null },
    "../../components/BackupRenameAction": { BackupRenameAction: () => null },
    "../../components/InlineConfirmAction": { InlineConfirmAction: () => null }
  });
  const backup = { backup_id: "snapshot", backup_path: "C:/fixture/snapshot", backup_kind: "manual",
    created_at_unix_ms: 1, file_count: 1, total_bytes: 32 };
  for (const [readOnly, canRestoreBackup, handler, disabled] of [
    [false, true, true, false], [true, true, true, true],
    [false, false, true, true], [false, true, false, true]
  ]) {
    const markup = renderToStaticMarkup(React.createElement(BackupTable, {
      backups: [backup], instanceId: "fixture", locale: "en-US", t,
      readOnly, canRestoreBackup, onRestoreBackup: handler ? () => {} : undefined
    }));
    const restore = [...markup.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)]
      .find(([, , content]) => content.includes("servers.backups.restore"));
    assert.ok(restore, "The restore action is visible");
    assert.equal(/\bdisabled(?:=|\s|$)/.test(restore[1]), disabled,
      JSON.stringify({ readOnly, canRestoreBackup, handler }));
  }
});

test("retired section ids resolve to their consolidated entries", () => {
  const { resolveMaintenanceSectionId } = load("views/servers/MaintenanceWorkspace.tsx");
  assert.equal(resolveMaintenanceSectionId("save-import"), "saves");
  assert.equal(resolveMaintenanceSectionId("save-policy"), "backups");
  assert.equal(resolveMaintenanceSectionId("storage"), "runtime", "directories merged into Runtime");
  for (const id of ["saves", "backups", "runtime", "cluster", "broadcast"]) {
    assert.equal(resolveMaintenanceSectionId(id), id);
  }
});

test("instance backups and cluster snapshots share one size formatter", () => {
  const { formatStorageSize } = load("views/servers/format-storage-size.ts");
  assert.equal(formatStorageSize(512), "512 B");
  assert.equal(formatStorageSize(2048), "2 KB");
  assert.equal(formatStorageSize(1572864), "1.5 MB");
  assert.equal(formatStorageSize(3 * 1024 ** 3), "3.00 GB");
  assert.equal(formatStorageSize(-1), "—");
  const source = (file) => fs.readFileSync(path.join(__dirname, "../src/views/servers", file), "utf8");
  for (const file of ["BackupTable.tsx", "ArkClusterBackupPanel.tsx", "SaveFileTable.tsx", "SaveSlotTable.tsx"]) {
    assert.match(source(file), /formatStorageSize\(/, `${file} must use the shared size formatter`);
  }
  assert.doesNotMatch(source("ArkClusterBackupPanel.tsx"), /1048576|MiB/);
  assert.doesNotMatch(source("BackupTable.tsx"), /function formatBackupSize/);
});

test("the save file listing is requested by instance and backup id, never by a UI path", () => {
  const api = fs.readFileSync(path.join(__dirname, "../src/api.ts"), "utf8");
  assert.match(api, /invokeOrMock<SaveFileListing>\("list_instance_save_files", \{ instanceId, backupId \}\)/);
  const source = (file) => fs.readFileSync(path.join(__dirname, "../src/views/servers", file), "utf8");
  assert.match(source("use-save-file-listing.ts"), /listInstanceSaveFiles\(instanceId, backupId\)/);
  assert.match(source("SaveFileBrowser.tsx"), /useSaveFileListing\(instanceId, backupId\)/);
  assert.match(source("SaveSlotTable.tsx"), /useSaveFileListing\(instanceId, null\)/);
});

function slotFixture() {
  const entry = (relative_path, is_directory, size_bytes = 0) => ({ relative_path, is_directory, size_bytes, modified_at_unix_ms: 1800000000000 });
  return {
    instance_id: "fixture", backup_id: null, root_path: "D:\\Saves", exists: true, truncated: false, skipped_links: [],
    slot_model: "native",
    entries: [entry("Multiplayer/Slot2", true), entry("Multiplayer/Slot2/__RESUME__", false, 6),
      entry("Multiplayer/Slot4", true), entry("Multiplayer/Slot4/__RESUME__", false, 9), entry("notes.txt", false, 1)],
    slots: [
      { id: "d:Multiplayer/Slot2", name: "Slot2", label: { kind: "save_slot", value: "2" }, kind: "native", paths: ["Multiplayer/Slot2"],
        size_bytes: 6, file_count: 1, modified_at_unix_ms: 1800000000000, current: true },
      { id: "d:Multiplayer/Slot4", name: "Slot4", label: { kind: "save_slot", value: "4" }, kind: "native", paths: ["Multiplayer/Slot4"],
        size_bytes: 9, file_count: 1, modified_at_unix_ms: null, current: false },
      { id: "t:", name: "", label: null, kind: "files", paths: ["notes.txt"], size_bytes: 1, file_count: 1, modified_at_unix_ms: 1, current: false }
    ]
  };
}

function loadSlotTable(listing, lan = false) {
  return load("views/servers/SaveSlotTable.tsx", {
    "@tauri-apps/api/core": { isTauri: () => !lan },
    "../../api-transport": { shouldUseLanApi: () => lan },
    "../../view-models": { formatTime: () => "TIME", formatRelativeTime: () => "RELATIVE" },
    "./use-save-file-listing": { useSaveFileListing: () => ({ listing, error: null, loading: false, reload: async () => {} }) }
  });
}

test("save slots render as read-only backup-style rows with the current slot marked", () => {
  const { renderToStaticMarkup } = require("react-dom/server");
  const React = require("react");
  const listing = slotFixture();
  const { SaveSlotTable, slotFolder, slotName } = loadSlotTable(listing);
  const opened = [];
  const html = renderToStaticMarkup(React.createElement(SaveSlotTable, { instanceId: "fixture", locale: "zh-CN", t,
    onOpenLocalPath: (value) => opened.push(value) }));
  assert.equal((html.match(/class="server-file-backup-row"/g) ?? []).length, 3, "one shared-style row per slot");
  assert.equal((html.match(/data-current="true"/g) ?? []).length, 1);
  assert.match(html, /servers\.saveSlots\.current/);
  assert.match(html, /servers\.saveSlots\.label\.save_slot/);
  assert.match(html, /servers\.saveSlots\.neverSaved/);
  assert.match(html, /servers\.saveSlots\.looseFiles/);
  assert.match(html, /aria-expanded="false"/);
  assert.doesNotMatch(html, /servers\.backups\.(restore|delete)|server-backup-rename/, "save slots are read-only");
  assert.equal(slotFolder(listing, listing.slots[0]), "D:\\Saves\\Multiplayer\\Slot2");
  assert.equal(slotFolder(listing, listing.slots[2]), "D:\\Saves");
  assert.equal(slotName(listing.slots[2], t), "servers.saveSlots.looseFiles");
  const { entriesUnder } = load("views/servers/SaveFileTable.tsx", { "../../view-models": { formatTime: () => "TIME" } });
  assert.deepEqual(entriesUnder(listing.entries, listing.slots[0].paths).map((entry) => entry.relative_path),
    ["Multiplayer/Slot2", "Multiplayer/Slot2/__RESUME__"], "Slot2 must not include Slot4 or loose files");
  const fallback = renderToStaticMarkup(React.createElement(loadSlotTable({ ...listing, slot_model: "fallback" }).SaveSlotTable,
    { instanceId: "fixture", locale: "zh-CN", t }));
  assert.match(fallback, /servers\.saveSlots\.fallbackNote/);
  const lan = renderToStaticMarkup(React.createElement(loadSlotTable(listing, true).SaveSlotTable, { instanceId: "fixture", locale: "zh-CN", t }));
  assert.match(lan, /servers\.saveFiles\.localOnly/);
  assert.doesNotMatch(lan, /server-file-backup-row/);
});

test("ARK cluster start/stop and cluster snapshots lock each other across categories", () => {
  const cleanups = [];
  const react = { useCallback: (fn) => fn, useSyncExternalStore: (_subscribe, read) => read(),
    useEffect: (effect) => { const cleanup = effect(); if (cleanup) cleanups.push(cleanup); } };
  const { useArkClusterActivity } = load("views/servers/ark-cluster-activity.ts", { react });
  assert.equal(useArkClusterActivity("ark-a", "snapshot", false), false);
  useArkClusterActivity("ark-a", "operation", true);
  assert.equal(useArkClusterActivity("ark-a", "snapshot", false), true, "start/stop must lock snapshots");
  assert.equal(useArkClusterActivity("ark-b", "snapshot", false), false, "another cluster stays unlocked");
  useArkClusterActivity("ark-a", "operation", false);
  assert.equal(useArkClusterActivity("ark-a", "snapshot", true), false);
  assert.equal(useArkClusterActivity("ark-a", "operation", false), true, "snapshot work must lock start/stop");
  cleanups.forEach((cleanup) => cleanup());
  assert.equal(useArkClusterActivity("ark-a", "operation", false), false, "unmounting releases the lock");

  const source = (name) => fs.readFileSync(path.join(__dirname, "../src/views/servers", name), "utf8");
  assert.match(source("ArkClusterPanel.tsx"), /useArkClusterActivity\(instanceId, "operation", !!busy\)/);
  assert.match(source("ArkClusterPanel.tsx"), /disabled=\{loading \|\| !!busy \|\| snapshotBusy \|\| report\.start_blocked\}/);
  assert.match(source("ArkClusterMaintenance.tsx"), /const snapshotBusy = loading \|\| working \|\| backupBusy \|\| !!pending \|\| !!inspectionError;/);
  assert.match(source("ArkClusterMaintenance.tsx"), /useArkClusterActivity\(instanceId, "snapshot", snapshotBusy\)/);
});
