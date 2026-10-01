const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}

// Register styles after source extensions so extensionless imports resolve TypeScript first.
require.extensions[".css"] = (module) => module._compile("", module.filename);

const filename = path.resolve(__dirname, "../src/views/servers/ModWorkbench.tsx");
const source = fs.readFileSync(filename, "utf8");
const syntax = parseSource(source, filename);
const model = require("../src/views/servers/mod-workbench-model.ts");
const plans = require("../src/views/servers/mod-workbench-plans.ts");
const collections = require("../src/views/servers/mod-workbench-collections.ts");
const patch = require("../src/views/servers/mod-settings-patch.ts");
const dstPolicy = require("../src/views/servers/mod-workbench-dst-policy.ts");
const workshopControls = require("../src/views/servers/mod-workbench-workshop-controls.ts");
const workshopInventory = require("../src/views/servers/mod-workbench-workshop-inventory.ts");
const { parseSettingsObject, parseWorkshopIdList } = require("../src/views/settings/guided-settings.ts");

function functionSource(name) {
  let result;
  visitSyntax(syntax, (node) => {
    if (node.type === "FunctionDeclaration" && node.identifier?.value === name) {
      assert.equal(result, undefined, `multiple ${name} declarations`);
      result = sourceText(source, node);
    }
  });
  assert.ok(result, `missing ${name}`);
  return result;
}

function deferred() {
  let resolve;
  const promise = new Promise((accept) => { resolve = accept; });
  return { promise, resolve };
}

const COLLECTION_ID = "9000001";
const MEMBER_ID = "2000001";
const EXTERNAL_COLLECTION = { id: "9000002", title: "Added elsewhere", member_ids: ["2000002"] };
const INITIAL_SETTINGS = {
  workshop_items: "", mods: "", map_name: "Muldraugh, KY", public_name: "Original name",
  steam_workshop_collections: []
};

function fixture() {
  let persisted = structuredClone(INITIAL_SETTINGS);
  const scanStarted = deferred();
  const releaseScan = deferred();
  const state = { writes: [], downloads: [], downloadError: null, downloadState: "idle" };
  const exports = {};
  const root = {
    id: COLLECTION_ID, title: "Installed collection", status: "resolved", item_kind: "collection",
    consumer_app_id: 108600, child_count: 1,
    children: [{ id: MEMBER_ID, status: "resolved", item_kind: "item", consumer_app_id: 108600 }]
  };
  const noOp = () => {};
  const context = {
    ...model, ...plans, ...collections, ...patch, parseSettingsObject, exports, Set, Map,
    moduleId: "projectzomboid", expectedAppId: 108600, locale: "en-US", readOnly: false,
    workflow: { steamDownloadMode: "steamcmd-cache" },
    modMutationsDisabled: false, modWorkflowUnsupported: false,
    modChangesBlockedRef: { current: false },
    manualEnablement: null, manualStaging: null,
    settings: structuredClone(INITIAL_SETTINGS), latestSettingsRef: { current: structuredClone(INITIAL_SETTINGS) },
    lookupMap: { [COLLECTION_ID]: root }, resolvedLookupMap: { [COLLECTION_ID]: root },
    t: (key, _params, fallback) => fallback ?? key,
    describeError: (error) => error instanceof Error ? error.message : String(error),
    startTransition: (operation) => operation(),
    // React coordination is outside this regression; execute its real owned operation.
    runModMutation: async (_kind, operation) => operation(),
    setInstallingWorkshopIds: noOp, setDownloadResult: noOp, setApplyMessage: noOp,
    setPzSnapshot: noOp, setScanNonce: noOp,
    setDownloadState: (value) => { state.downloadState = value; },
    setDownloadError: (value) => { state.downloadError = value; },
    readInstanceDetails: async (id) => {
      assert.equal(id, "instance-pz");
      return {
        summary: { id, status: "Stopped", bind_ip: "127.0.0.1" }, active_run: null,
        auto_backup_on_stop: false, backup_retention_count: 3, ports: [],
        settings_json: JSON.stringify(persisted)
      };
    },
    downloadSteamWorkshopItems: async (id, ids) => {
      assert.equal(id, "instance-pz");
      state.downloads.push([...ids]);
      return { items: ids.map((item_id) => ({ item_id, expected_path_exists: true })) };
    },
    readProjectZomboidWorkshopModsSnapshot: async (id, ids) => {
      assert.equal(id, "instance-pz");
      assert.equal(ids.join(","), MEMBER_ID);
      scanStarted.resolve();
      await releaseScan.promise;
      return { items: [{ workshop_item_id: MEMBER_ID, mods: [{ mod_id: "FixtureMod", map_ids: [] }] }] };
    },
    props: {
      details: { summary: { id: "instance-pz" } },
      onSaveSettings: async (input, options) => {
        assert.equal(input.id, "instance-pz");
        // The external persistence boundary enforces the real handler's CAS token.
        assert.equal(options.expectedSettingsJson, JSON.stringify(persisted));
        assert.equal(options.throwOnError, true);
        state.writes.push(JSON.parse(input.settings_json));
        persisted = JSON.parse(input.settings_json);
      }
    }
  };
  const declarations = [
    "readWritableInstanceState", "persistSettings", "isCollectionSummary",
    "canInstallWorkshopItems", "handleInstallWorkshopItems"
  ].map(functionSource).join("\n");
  vm.runInNewContext(transpileTypeScript(`${declarations}\nexports.install = handleInstallWorkshopItems;`, filename), context, { filename });
  return {
    state,
    install: () => exports.install([COLLECTION_ID]),
    scanStarted: scanStarted.promise,
    finishScan: () => releaseScan.resolve(),
    externalUpdate: (next) => {
      persisted = structuredClone(next);
      // Mirrors the parent details refresh effect while the local scan is awaited.
      context.latestSettingsRef.current = structuredClone(next);
    },
    persisted: () => structuredClone(persisted)
  };
}

test("PZ collection install preserves unrelated changes arriving while its local metadata scan is pending", async () => {
  const f = fixture();
  const installation = f.install();
  await f.scanStarted;
  f.externalUpdate({ ...INITIAL_SETTINGS, public_name: "Renamed elsewhere", independent_setting: 42 });
  f.finishScan();
  assert.equal(await installation, true);
  assert.equal(f.state.downloadState, "success");
  assert.equal(f.state.downloadError, null);
  assert.equal(f.state.writes.length, 1);
  const saved = f.persisted();
  assert.equal(saved.public_name, "Renamed elsewhere");
  assert.equal(saved.independent_setting, 42);
  assert.equal(saved.workshop_items, MEMBER_ID);
  assert.equal(saved.mods, "FixtureMod");
  assert.deepEqual(saved.steam_workshop_collections, [
    { id: COLLECTION_ID, title: "Installed collection", member_ids: [MEMBER_ID] }
  ]);
});

test("PZ collection install reports a source conflict instead of replacing a collection added during its scan", async () => {
  const f = fixture();
  const installation = f.install();
  await f.scanStarted;
  const external = {
    ...INITIAL_SETTINGS, public_name: "Renamed elsewhere", steam_workshop_collections: [EXTERNAL_COLLECTION]
  };
  f.externalUpdate(external);
  f.finishScan();
  assert.equal(await installation, false);
  assert.equal(f.state.downloadState, "error");
  assert.match(f.state.downloadError, /collections changed elsewhere/i);
  assert.equal(f.state.writes.length, 0, "conflicts must be rejected before a save request");
  assert.deepEqual(f.persisted(), external);
  assert.deepEqual(f.state.downloads, [[MEMBER_ID]], "download completion alone must not commit provenance");
});

function dstFixture() {
  const initial = { steam_workshop_collections: [{ id: COLLECTION_ID, title: "Owned collection", member_ids: [MEMBER_ID] }],
    public_name: "Original name" };
  let persisted = structuredClone(initial);
  const secondReadStarted = deferred();
  const releaseSecondRead = deferred();
  let readCount = 0;
  let pending;
  const state = { writes: [] };
  const exports = {};
  const noOp = () => {};
  const context = {
    ...model, ...plans, ...collections, ...patch, ...dstPolicy, parseSettingsObject, exports, Set, Map,
    moduleId: "dontstarve", expectedAppId: 322330, locale: "en-US", pzSnapshot: null, lookupMap: {}, readOnly: false,
    modChangesBlockedRef: { current: false }, latestSettingsRef: { current: structuredClone(initial) },
    t: (key, _params, fallback) => fallback ?? key,
    startTransition: (operation) => operation(),
    launchModMutation: (_kind, operation) => { pending = operation(); },
    setApplyMessage: noOp, setScanNonce: noOp, setSelectedEnabledRowKey: noOp,
    readInstanceDetails: async (id) => {
      assert.equal(id, "instance-dst");
      readCount += 1;
      if (readCount === 2) {
        secondReadStarted.resolve();
        await releaseSecondRead.promise;
      }
      return {
        summary: { id, status: "Stopped", bind_ip: "127.0.0.1" }, active_run: null,
        auto_backup_on_stop: false, backup_retention_count: 3, ports: [],
        settings_json: JSON.stringify(persisted)
      };
    },
    props: {
      details: { summary: { id: "instance-dst" } },
      onSaveSettings: async (input, options) => {
        assert.equal(input.id, "instance-dst");
        assert.equal(options.expectedSettingsJson, JSON.stringify(persisted));
        assert.equal(options.throwOnError, true);
        state.writes.push(JSON.parse(input.settings_json));
        persisted = JSON.parse(input.settings_json);
      }
    }
  };
  const declarations = ["readWritableInstanceState", "persistSettings", "assertDstModOwnership",
    "handleSetDstModsEnabled", "handleRemoveWorkshopItems"].map(functionSource).join("\n");
  vm.runInNewContext(transpileTypeScript(`${declarations}\nexports.enable = handleSetDstModsEnabled; exports.remove = handleRemoveWorkshopItems;`, filename), context, { filename });
  return {
    initial, state, secondReadStarted: secondReadStarted.promise,
    start: (action) => {
      if (action === "remove") exports.remove([MEMBER_ID]);
      else exports.enable([MEMBER_ID], action === "enable");
      assert.ok(pending, "the actual handler must launch an owned mutation");
      return pending;
    },
    externalUpdate: (next) => { persisted = structuredClone(next); },
    finishSecondRead: () => releaseSecondRead.resolve(),
    persisted: () => structuredClone(persisted)
  };
}

test("DST member enable, disable and remove refuse newly introduced raw Lua at the final settings read", async () => {
  for (const action of ["enable", "disable", "remove"]) {
    for (const shard of ["master", "caves"]) {
      const f = dstFixture();
      const operation = f.start(action);
      const rejected = assert.rejects(operation, /modoverrides|raw|Lua/i, `${action}: ${shard} raw override must block saving`);
      await f.secondReadStarted;
      const external = { ...f.initial, [`${shard}_modoverrides_lua`]: "return {custom={enabled=true}}" };
      f.externalUpdate(external);
      f.finishSecondRead();
      await rejected;
      assert.equal(f.state.writes.length, 0, `${action} must not save over new raw Lua`);
      assert.deepEqual(f.persisted(), external);
    }
  }
});

test("DST member enable, disable and remove cannot revive ownership removed during the final settings read", async () => {
  for (const action of ["enable", "disable", "remove"]) {
    const f = dstFixture();
    const operation = f.start(action);
    const rejected = assert.rejects(operation, /not configured|ownership|owned/i, `${action}: saved ownership must still exist`);
    await f.secondReadStarted;
    const external = { ...f.initial, steam_workshop_collections: [] };
    f.externalUpdate(external);
    f.finishSecondRead();
    await rejected;
    assert.equal(f.state.writes.length, 0, `${action} must not recreate a removed instance member`);
    assert.deepEqual(f.persisted(), external);
  }
});

test("DST member controls still merge unrelated settings changed during the final read", async () => {
  for (const action of ["enable", "disable", "remove"]) {
    const f = dstFixture();
    const operation = f.start(action);
    await f.secondReadStarted;
    f.externalUpdate({ ...f.initial, public_name: "Renamed elsewhere", independent_setting: 42 });
    f.finishSecondRead();
    await operation;
    assert.equal(f.state.writes.length, 1);
    const saved = f.persisted();
    assert.equal(saved.public_name, "Renamed elsewhere");
    assert.equal(saved.independent_setting, 42);
    assert.deepEqual(saved.steam_workshop_collections, f.initial.steam_workshop_collections);
    assert.equal(saved.master_enabled_workshop_mod_ids, action === "enable" ? MEMBER_ID : "");
    assert.equal(saved.caves_enabled_workshop_mod_ids, action === "enable" ? MEMBER_ID : "");
    assert.equal(saved.shared_workshop_mod_ids, action === "remove" ? "" : MEMBER_ID);
    if (action === "remove") assert.deepEqual(saved.dst_removed_workshop_mod_ids, [MEMBER_ID]);
  }
});

function controlFixture(moduleId, enabled) {
  const initial = { public_name: "Original name",
    steam_workshop_collections: [{ id: COLLECTION_ID, title: "Saved", member_ids: [MEMBER_ID] }],
    ...(moduleId === "projectzomboid" ? { workshop_items: MEMBER_ID, mods: enabled ? "FixtureMod" : "", map_name: "Muldraugh, KY" }
      : moduleId === "palworld" ? { mod_package_names: enabled ? "FixturePackage" : "" }
      : moduleId === "squad" ? {}
      : { mod_workshop_ids: enabled ? MEMBER_ID : "", steam_workshop_disabled_mod_ids: enabled ? [] : [MEMBER_ID] }) };
  let persisted = structuredClone(initial), readCount = 0, pending;
  const secondReadStarted = deferred(), releaseSecondRead = deferred();
  const state = { writes: [], saveOptions: [], metadataReads: [] }, exports = {}, noOp = () => {};
  const instanceId = `instance-${moduleId}`;
  const context = {
    ...model, ...plans, ...collections, ...patch, ...workshopControls, ...workshopInventory,
    parseSettingsObject, parseWorkshopIdList, exports, Set, Map, moduleId,
    expectedAppId: null, locale: "en-US", pzSnapshot: null, lookupMap: {}, readOnly: false,
    modChangesBlockedRef: { current: false }, latestSettingsRef: { current: structuredClone(initial) },
    t: (key, _params, fallback) => fallback ?? key,
    describeError: (error) => error instanceof Error ? error.message : String(error),
    startTransition: (operation) => operation(),
    launchModMutation: (_kind, operation) => { pending = operation(); },
    setApplyMessage: noOp, setScanNonce: noOp, setPzSnapshot: noOp,
    readInstanceDetails: async (id) => {
      assert.equal(id, instanceId, "every final read belongs to the initiating instance");
      if (++readCount === 2) { secondReadStarted.resolve(); await releaseSecondRead.promise; }
      return { summary: { id, status: "Stopped", bind_ip: "127.0.0.1" }, active_run: null,
        auto_backup_on_stop: false, backup_retention_count: 3, ports: [], settings_json: JSON.stringify(persisted) };
    },
    readProjectZomboidWorkshopModsSnapshot: async (id, ids) => {
      assert.equal(id, instanceId);
      state.metadataReads.push([...ids]);
      return { workshop_root_exists: true, items: [{ workshop_item_id: MEMBER_ID, status: "installed",
        mods: [{ status: "loaded", mod_id: "FixtureMod", map_ids: [] }] }] };
    },
    readManualModInventory: async (id) => {
      assert.equal(id, instanceId);
      state.metadataReads.push([MEMBER_ID]);
      return { module_id: "palworld", target_exists: true, target_path: `C:\\${instanceId}\\Mods`,
        items: [{ name: MEMBER_ID, path: `C:\\${instanceId}\\Mods\\${MEMBER_ID}`, file_count: 1,
          inferred_id: "FixturePackage", item_type: "directory" }] };
    },
    props: {
      details: { summary: { id: instanceId } },
      onSaveSettings: async (input, options) => {
        assert.equal(input.id, instanceId);
        assert.equal(options.expectedSettingsJson, JSON.stringify(persisted), "CAS uses the final read");
        assert.equal(options.throwOnError, true);
        state.writes.push(JSON.parse(input.settings_json));
        state.saveOptions.push(JSON.parse(JSON.stringify(options)));
        persisted = JSON.parse(input.settings_json);
      }
    }
  };
  const declarations = ["readWritableInstanceState", "persistSettings", "assertWorkshopControlBase", "workshopControlError",
    "handleRemoveWorkshopItems", "handleRemoveManagedWorkshopMembers", "handleSetCollectionMembersEnabled"].map(functionSource).join("\n");
  vm.runInNewContext(transpileTypeScript(`${declarations}\nexports.enable = handleSetCollectionMembersEnabled;
    exports.remove = handleRemoveManagedWorkshopMembers;`, filename), context, { filename });
  return { initial, state, secondReadStarted: secondReadStarted.promise,
    start: (action) => {
      if (action === "remove") exports.remove([MEMBER_ID], COLLECTION_ID);
      else exports.enable([MEMBER_ID], action === "enable");
      assert.ok(pending, "the actual handler must launch a mutation");
      return pending;
    },
    externalUpdate: (next) => { persisted = structuredClone(next); },
    finishSecondRead: () => releaseSecondRead.resolve(), persisted: () => structuredClone(persisted) };
}

for (const moduleId of ["barotrauma", "conanexiles", "soulmask"]) {
  test(`${moduleId}: removal rejects both enablement migrations at its final read without saving`, async () => {
    for (const enabled of [false, true]) {
      const f = controlFixture(moduleId, enabled);
      const operation = f.start("remove");
      const rejected = assert.rejects(operation, /collections changed elsewhere/i);
      await f.secondReadStarted;
      const external = { ...f.initial, mod_workshop_ids: enabled ? "" : MEMBER_ID,
        steam_workshop_disabled_mod_ids: enabled ? [MEMBER_ID] : [] };
      f.externalUpdate(external); f.finishSecondRead(); await rejected;
      assert.equal(f.state.writes.length, 0, `${enabled ? "active to disabled" : "disabled to active"} must reject before IPC`);
      assert.deepEqual(f.persisted(), external);
    }
  });
  test(`${moduleId}: removing either enabled or disabled members merges unrelated changes once`, async () => {
    for (const enabled of [false, true]) {
      const f = controlFixture(moduleId, enabled), operation = f.start("remove");
      await f.secondReadStarted;
      f.externalUpdate({ ...f.initial, public_name: "External name", independent_setting: 42 });
      f.finishSecondRead(); await operation;
      assert.equal(f.state.writes.length, 1);
      assert.equal(f.persisted().public_name, "External name");
      assert.equal(f.persisted().independent_setting, 42);
      assert.deepEqual(workshopControls.readOwnedWorkshopModIds(moduleId, f.persisted()), []);
      assert.deepEqual(f.persisted().steam_workshop_collections, f.initial.steam_workshop_collections);
    }
  });
}

for (const moduleId of ["projectzomboid", "palworld"]) {
  test(`${moduleId}: actual enable, disable and remove handlers reject changed control settings at the final read`, async () => {
    for (const action of ["enable", "disable", "remove"]) {
      const f = controlFixture(moduleId, action !== "enable"), operation = f.start(action);
      const rejected = assert.rejects(operation, /collections changed elsewhere/i);
      await f.secondReadStarted;
      const external = { ...f.initial, [moduleId === "projectzomboid" ? "mods" : "mod_package_names"]: "AddedElsewhere" };
      f.externalUpdate(external); f.finishSecondRead(); await rejected;
      assert.equal(f.state.writes.length, 0, action);
      assert.deepEqual(f.state.metadataReads, [[MEMBER_ID]], "uses fresh metadata for the initiating instance");
      assert.deepEqual(f.persisted(), external);
    }
  });
  test(`${moduleId}: actual member controls preserve unrelated final-read changes in one save`, async () => {
    for (const action of ["enable", "disable", "remove"]) {
      const f = controlFixture(moduleId, action !== "enable"), operation = f.start(action);
      await f.secondReadStarted;
      f.externalUpdate({ ...f.initial, public_name: "External name", independent_setting: 42 });
      f.finishSecondRead(); await operation;
      assert.equal(f.state.writes.length, 1, action);
      assert.equal(f.persisted().public_name, "External name");
      assert.equal(f.persisted().independent_setting, 42);
      const key = moduleId === "projectzomboid" ? "mods" : "mod_package_names";
      assert.equal(f.persisted()[key], action === "enable" ? (moduleId === "projectzomboid" ? "FixtureMod" : "FixturePackage") : "");
      if (action === "remove" && moduleId === "palworld") assert.deepEqual(f.persisted().steam_workshop_removed_mod_ids, [MEMBER_ID]);
    }
  });
}

test("Squad member removal reaches its dedicated transaction with unchanged settings and a fresh CAS token", async () => {
  const f = controlFixture("squad", true), operation = f.start("remove");
  await f.secondReadStarted;
  const external = { ...f.initial, public_name: "External name", independent_setting: 42 };
  f.externalUpdate(external); f.finishSecondRead(); await operation;
  assert.equal(f.state.writes.length, 1, "zero dirty keys must still execute file removal");
  assert.deepEqual(f.persisted(), external);
  assert.deepEqual(f.state.saveOptions[0].collectionRemoval, { collectionId: COLLECTION_ID, memberIds: [MEMBER_ID], retainCollection: true });
});
