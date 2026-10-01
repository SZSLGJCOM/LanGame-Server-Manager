const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { mountWorkbench, settle } = require("./helpers/runtime-workbench.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (loaded, filename) => {
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = () => {};
const t = (key, _params, fallback) => fallback || key;
const i18n = { useI18n: () => ({ locale: "en-US", t }), selectLocaleText: (_locale, _zh, en) => en };

function load(relative, mocks = {}) {
  const filename = path.join(__dirname, "../src", relative);
  const originalRequire = Module.createRequire(filename);
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  loaded.require = (specifier) => Object.hasOwn(mocks, specifier) ? mocks[specifier] : originalRequire(specifier);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  return loaded.exports;
}

function hooks() {
  const effects = [];
  return { effects, react: {
    useEffect: (effect) => effects.push(effect),
    useMemo: (read) => read(),
    useRef: (current) => ({ current }),
    useState: (initial) => [typeof initial === "function" ? initial() : initial, () => {}]
  } };
}

function findElement(value, predicate) {
  if (Array.isArray(value)) return value.flatMap((child) => findElement(child, predicate));
  if (!value || typeof value !== "object" || !value.props) return [];
  return [...(predicate(value) ? [value] : []), ...findElement(value.props.children, predicate)];
}

test("the shared runtime console displays only retained archive output and creates no native lifecycle", async (context) => {
  let commands = 0;
  const archive = {
    runs: { entries: [{ id: 9, status: "running" }], total: 1, truncated: false },
    log: { relative_path: "logs/retained.log", text: "retained line 1\nretained line 2", truncated: true, issues: [] }
  };
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    archive, commandDraft: "native command must not run", startupPending: true,
    details: { summary: { id: "fixture", module_id: "dontstarve", status: "running", active_process_count: 2 },
      active_run: { run_id: 4, processes: [{ process_key: "caves", run_id: 5, is_primary: false, status: "running" }] } },
    onSendRuntimeCommand: async () => { commands += 1; return null; }
  });
  context.after(mounted.dispose);
  assert.deepEqual(mounted.registrations, [], "An archive must not subscribe to native runtime events");
  const [consoleElement] = findElement(mounted.tree, (element) => element.type === "pre");
  assert.equal(consoleElement.props.children[0], archive.log.text);
  assert.equal(consoleElement.props.title, "The log display is truncated.");
  const [commandInput] = findElement(mounted.tree, (element) => element.type === "input");
  assert.equal(commandInput.props.disabled, true);
  const [form] = findElement(mounted.tree, (element) => element.type === "form");
  form.props.onSubmit({ preventDefault() {} });
  await settle();
  assert.equal(commands, 0, "Even a programmatic form submission must not send a command");
});

const rosterModel = load("views/servers/player-center/player-access-roster-model.ts");
function moduleDetails() {
  return { summary: { id: "fixture" }, schema_json: JSON.stringify({ type: "object", properties: {
    admin_ids: { type: "array", items: { type: "string" }, default: ["schema-admin"], "x-lsgm-player-access-kind": "admin" },
    whitelist_ids: { type: "array", items: { type: "string" }, default: ["schema-player"], "x-lsgm-player-access-kind": "allow" }
  } }) };
}

test("shared roster fields preserve normal defaults and use only actual saved values in archives", () => {
  const settings = Object.assign(Object.create({ whitelist_ids: ["inherited-player"] }), { admin_ids: ["saved-admin"] });
  const ordinary = rosterModel.buildRosterFields(moduleDetails(), "en-US", t, settings);
  assert.deepEqual(ordinary.map((field) => field.entries.map((entry) => entry.rawValue)), [["saved-admin"], ["schema-player"]]);
  const archived = rosterModel.buildRosterFields(moduleDetails(), "en-US", t, settings, { savedValuesOnly: true });
  assert.deepEqual(archived.map((field) => field.key), ["admin_ids"]);
  assert.deepEqual(archived[0].entries.map((entry) => entry.rawValue), ["saved-admin"]);
});

test("the shared player-access hook hard rejects mutations while the archive roster remains selectable", async () => {
  const harness = hooks();
  const { usePlayerAccess } = load("views/servers/player-center/use-player-access.tsx", {
    react: harness.react, "../../../i18n": i18n,
    "../../../app-state": { describeError: String },
    "../../../components/ActivityNotice": { ActivityNotice: () => null }
  });
  let writes = 0;
  const access = usePlayerAccess({
    details: { summary: { id: "fixture" }, settings_json: JSON.stringify({ admin_ids: ["saved-admin"] }) },
    moduleDetails: moduleDetails(), readOnly: true,
    onApplyPlayerAccessMutation: async () => { writes += 1; throw new Error("Unexpected native mutation"); }
  });
  assert.equal(access.disabled, true);
  assert.deepEqual(access.fields.map((field) => field.key), ["admin_ids"]);
  assert.equal(await access.onMutate(access.fields[0], "remove", "saved-admin"), false);
  assert.equal(await access.onMutate(access.fields[0], "add", "new-admin"), false);
  assert.equal(writes, 0);
});

test("an inactive live-player hook has no reads, refresh controller, or visibility subscription", async () => {
  const harness = hooks();
  let reads = 0;
  let controllers = 0;
  const { useLivePlayers } = load("views/servers/player-center/use-live-players.ts", {
    react: harness.react,
    "../../../app-state": { describeError: String },
    "../../../api": { readInstanceLivePlayers: () => { reads += 1; }, refreshInstanceLivePlayers: () => { reads += 1; } },
    "../../../domain/live-player-refresh": { LivePlayerRefreshController: class { constructor() { controllers += 1; } } }
  });
  const live = useLivePlayers("fixture", false, "stopped");
  const cleanups = harness.effects.map((effect) => effect());
  await live.refresh();
  assert.equal(reads, 0);
  assert.equal(controllers, 0);
  assert.equal(live.snapshot, null);
  for (const cleanup of cleanups) cleanup?.();
});

function maintenanceView(backups, archive) {
  const mocks = { "../../api": {}, "../../ark-clusters": { isArkModule: () => false } };
  // The shared parent owns the backup loading/empty-state decision. Native and
  // interactive child workbenches are outside this presentation contract.
  for (const name of ["AiBroadcastWorkbench", "ArkClusterMaintenance", "ArkClusterPanel", "DstWorldImportPanel",
    "ImmediateWorldSave", "InstanceAutostartEditor", "InstanceIsolationPanel", "InstancePanelReadStatus",
    "InstanceProgramMaintenance", "MaintenanceWorkspace", "RuntimePerformanceEditor", "RuntimeRecoveryEditor", "SavePolicyEditor"]) {
    mocks[`./${name}`] = { [name]: () => null };
  }
  const { ServerMaintenanceWorkspace } = load("views/servers/ServerMaintenanceWorkspace.tsx", mocks);
  const root = ServerMaintenanceWorkspace({ active: true, locale: "en-US", t,
    details: { summary: { id: "fixture", module_id: "minecraft", status: "stopped", active_process_count: 0 },
      config_file_path: "D:/fixture/config/instance.json", backup_uses_declared_saves_path: true },
    moduleDetails: null, selectedBackups: backups, archive });
  return root.props.sections.find((section) => section.id === "backups").content;
}

test("unreadable or truncated archive backup metadata cannot be reported as an empty backup history", () => {
  for (const retained of [{ entries: [], truncated: false, issues: ["Retained metadata is unreadable"] },
    { entries: [], truncated: true, issues: [] }]) {
    const content = maintenanceView([], { backups: retained, maintenance: { crash_restart_limit: 3 } });
    const counts = findElement(content, (element) => element.props.className === "server-file-backup-stat");
    assert.deepEqual(counts.map((element) => element.props.children[0].props.children), ["—", "—", "—"]);
    assert.deepEqual(findElement(content, (element) => element.type.name === "BackupTable"), []);
    assert.ok(findElement(content, (element) => element.type === "p" && element.props.role === "status").length > 0);
  }
});

test("a readable empty archive and a normal empty instance retain the shared empty backup list", () => {
  for (const archive of [undefined, { backups: { entries: [], truncated: false, issues: [] },
    maintenance: { crash_restart_limit: 3 } }]) {
    const content = maintenanceView([], archive);
    const counts = findElement(content, (element) => element.props.className === "server-file-backup-stat");
    assert.deepEqual(counts.map((element) => element.props.children[0].props.children), [0, 0, 0]);
    const [table] = findElement(content, (element) => element.type.name === "BackupTable");
    assert.deepEqual(table.props.backups, []);
  }
});
