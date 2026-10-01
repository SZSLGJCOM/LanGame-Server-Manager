const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");
const workspaceRoot = path.resolve(desktopRoot, "..", "..");

function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
}

require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;

function loadStorageInitializationModel() {
  const sourcePath = path.join(desktopRoot, "src", "storage-initialization.ts");
  const source = fs.readFileSync(sourcePath, "utf8");
  const transpiled = transpileTypeScript(source, sourcePath);
  const module = { exports: {} };
  vm.runInNewContext(transpiled, { module, exports: module.exports, require }, { filename: sourcePath });
  return module.exports;
}

function loadBootstrapInitializationModel() {
  const sourcePath = path.join(desktopRoot, "src", "bootstrap-initialization.ts");
  const source = fs.readFileSync(sourcePath, "utf8");
  const transpiled = transpileTypeScript(source, sourcePath);
  const module = { exports: {} };
  vm.runInNewContext(transpiled, { module, exports: module.exports, require }, { filename: sourcePath });
  return module.exports;
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function storageStatus(schemaVersion = 9) {
  return {
    database_path: "C:/data/db/lgs.db",
    migrations_path: "C:/app/migrations",
    app_log_path: "C:/data/logs/desktop-app.log",
    database_exists: true,
    schema_version: schemaVersion,
    migrations_applied: true
  };
}

function bootstrapResponse(storage = storageStatus(0)) {
  return {
    booted_at_unix_ms: 1,
    state: {
      settings: {},
      storage,
      snapshot: { running_instances: 0 },
      modules: [],
      instances: [],
      jobs: []
    }
  };
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

const model = loadStorageInitializationModel();
const bootstrapModel = loadBootstrapInitializationModel();

test("bootstrap initialization shares one complete flight for the same attempt", async () => {
  const bootstrapGate = deferred();
  const calls = {
    bootstrap: 0,
    ensure: 0,
    sync: 0,
    list: 0,
    version: 0,
    overlays: 0
  };
  const coreOrder = [];
  const logs = [];
  const readyStorage = storageStatus();
  const modules = [{ id: "minecraft" }];
  const instances = [{ id: "minecraft-main" }];
  const coordinator = bootstrapModel.createBootstrapInitializationCoordinator({
    bootstrapApp: () => {
      calls.bootstrap += 1;
      coreOrder.push("bootstrap");
      return bootstrapGate.promise;
    },
    ensureStorageReady: async () => {
      calls.ensure += 1;
      coreOrder.push("ensure");
      return readyStorage;
    },
    syncModulesToStorage: async () => {
      calls.sync += 1;
      coreOrder.push("sync");
      return modules;
    },
    listInstancesFromStorage: async () => {
      calls.list += 1;
      coreOrder.push("list");
      return instances;
    },
    fetchAppVersion: async () => {
      calls.version += 1;
      return "0.1.0";
    },
    fetchOverlayFamilies: async () => {
      calls.overlays += 1;
      return [];
    },
    logFrontendEvent: async (level, action) => {
      logs.push({ level, action });
    },
    describeError: (error) => String(error),
    now: () => 100
  });

  const first = coordinator.request(0);
  const second = coordinator.request(0);
  assert.strictEqual(first, second);

  let firstSubscriberActive = true;
  const deliveries = [];
  const firstDelivery = first.initialization.then((outcome) => {
    if (firstSubscriberActive) {
      deliveries.push("first");
    }
    return outcome;
  });
  firstSubscriberActive = false;
  const secondDelivery = second.initialization.then((outcome) => {
    deliveries.push(`second:${outcome.status}`);
    return outcome;
  });

  bootstrapGate.resolve(bootstrapResponse());
  const [outcome, metadata] = await Promise.all([
    secondDelivery,
    second.metadata,
    firstDelivery
  ]).then(([initialization, resolvedMetadata]) => [initialization, resolvedMetadata]);

  assert.deepEqual(calls, {
    bootstrap: 1,
    ensure: 1,
    sync: 1,
    list: 1,
    version: 1,
    overlays: 1
  });
  assert.deepEqual(coreOrder, ["bootstrap", "ensure", "sync", "list"]);
  assert.deepEqual(deliveries, ["second:ready"]);
  assert.equal(outcome.status, "ready");
  assert.deepEqual(plain(outcome.booted.state.storage), readyStorage);
  assert.deepEqual(plain(outcome.booted.state.modules), modules);
  assert.deepEqual(plain(outcome.booted.state.instances), instances);
  assert.deepEqual(plain(metadata), { appVersion: "0.1.0", overlays: [] });
  assert.equal(logs.filter((entry) => entry.action === "frontend.bootstrap.request").length, 1);
  assert.equal(logs.filter((entry) => entry.action === "frontend.bootstrap.success").length, 1);
});

test("bootstrap initialization starts a fresh flight for a retry attempt after failure", async () => {
  const calls = { bootstrap: 0, ensure: 0, sync: 0, list: 0 };
  const logs = [];
  const readyStorage = storageStatus();
  const coordinator = bootstrapModel.createBootstrapInitializationCoordinator({
    bootstrapApp: async () => {
      calls.bootstrap += 1;
      return bootstrapResponse();
    },
    ensureStorageReady: async () => {
      calls.ensure += 1;
      return readyStorage;
    },
    syncModulesToStorage: async () => {
      calls.sync += 1;
      if (calls.sync === 1) {
        throw new Error("database is locked");
      }
      return [];
    },
    listInstancesFromStorage: async () => {
      calls.list += 1;
      return [];
    },
    fetchAppVersion: async () => "0.1.0",
    fetchOverlayFamilies: async () => [],
    logFrontendEvent: async (level, action) => {
      logs.push({ level, action });
    },
    describeError: (error) => (error instanceof Error ? error.message : String(error)),
    now: () => 100
  });

  const failed = await coordinator.request(0).initialization;
  assert.equal(failed.status, "failed");
  assert.deepEqual(plain(failed.storage), readyStorage);

  const retried = await coordinator.request(1).initialization;
  assert.equal(retried.status, "ready");
  assert.deepEqual(calls, { bootstrap: 2, ensure: 2, sync: 2, list: 1 });
  assert.equal(logs.filter((entry) => entry.action === "frontend.bootstrap.request").length, 2);
  assert.equal(logs.filter((entry) => entry.action === "frontend.bootstrap.failed").length, 1);
  assert.equal(logs.filter((entry) => entry.action === "frontend.bootstrap.success").length, 1);
});

test("storage initialization keeps unread data behind explicit pending and failed states", () => {
  const initial = model.createStorageInitializationState();
  assert.deepEqual(plain(initial), { status: "pending", attempt: 0 });
  assert.equal(model.resolveStorageInitializationSurface(initial), "loading");

  const failed = model.reduceStorageInitializationState(initial, {
    type: "failed",
    attempt: 0,
    error: "migration 8 was previously applied but is missing",
    storage: {
      database_path: " C:/data/db/lgs.db ",
      migrations_path: "C:/app/migrations",
      app_log_path: " C:/data/logs/desktop-app.log ",
      database_exists: true,
      schema_version: 9,
      migrations_applied: false
    }
  });

  assert.deepEqual(plain(failed), {
    status: "failed",
    attempt: 0,
    error: "migration 8 was previously applied but is missing",
    databasePath: "C:/data/db/lgs.db",
    logPath: "C:/data/logs/desktop-app.log",
    logDirectory: "C:/data/logs"
  });
  assert.equal(model.resolveStorageInitializationSurface(failed), "failure");
});

test("retry advances the attempt and ignores stale async completions", () => {
  const initial = model.createStorageInitializationState();
  const failed = model.reduceStorageInitializationState(initial, {
    type: "failed",
    attempt: 0,
    error: "database unavailable",
    storage: null
  });
  const retrying = model.reduceStorageInitializationState(failed, { type: "retry" });

  assert.deepEqual(plain(retrying), { status: "pending", attempt: 1 });
  assert.strictEqual(
    model.reduceStorageInitializationState(retrying, { type: "ready", attempt: 0 }),
    retrying
  );
  assert.strictEqual(
    model.reduceStorageInitializationState(retrying, {
      type: "failed",
      attempt: 0,
      error: "stale failure",
      storage: null
    }),
    retrying
  );

  const ready = model.reduceStorageInitializationState(retrying, { type: "ready", attempt: 1 });
  assert.deepEqual(plain(ready), { status: "ready", attempt: 1 });
  assert.equal(model.resolveStorageInitializationSurface(ready), "content");
  assert.strictEqual(model.reduceStorageInitializationState(ready, { type: "retry" }), ready);
});

test("app log files resolve to deterministic directories without filesystem probing", () => {
  assert.equal(
    model.resolveParentDirectory("C:\\fixture-data\\LanGame\\logs\\desktop-app.log"),
    "C:\\fixture-data\\LanGame\\logs"
  );
  assert.equal(model.resolveParentDirectory("/var/log/langame/desktop-app.log"), "/var/log/langame");
  assert.equal(model.resolveParentDirectory("/desktop-app.log"), "/");
  assert.equal(model.resolveParentDirectory("C:\\desktop-app.log"), "C:\\");
  assert.equal(model.resolveParentDirectory("desktop-app.log"), null);
});

test("assistant failure state exposes no false zero counts, no data issues, and no data prompts", () => {
  const { buildAssistantViewModel } = require(path.join(desktopRoot, "src", "assistant-state.ts"));
  const { buildAssistantCapsuleModel } = require(path.join(desktopRoot, "src", "assistant-summary.ts"));
  const input = {
    aiSettings: {
      enabled: false,
      provider: "openai-compatible",
      model: "fixture-model",
      baseUrl: "https://api.openai.com/v1",
      apiKey: "",
      apiKeyStored: false
    },
    locale: "en-US",
    activeJobsCount: 0,
    activeView: "servers",
    bootstrap: {
      booted_at_unix_ms: 0,
      state: {
        settings: {},
        storage: {
          database_path: "C:/data/db/lgs.db",
          migrations_path: "C:/app/migrations",
          app_log_path: "C:/data/logs/desktop-app.log",
          database_exists: true,
          schema_version: 9,
          migrations_applied: false
        },
        modules: [],
        instances: [],
        jobs: [],
        snapshot: { running_instances: 0 }
      }
    },
    storageReady: false,
    libraryPage: "catalog",
    overlayNames: [],
    runtimeAutoRefreshPaused: false,
    runtimeRefreshIssue: null,
    selectedInstanceDetails: null,
    selectedInstanceId: null,
    selectedInstanceModuleDetails: null,
    selectedLaunchPlan: null,
    selectedLaunchPlanError: null,
    selectedLogDocument: null,
    selectedModuleDetails: null,
    selectedRuntime: null,
    serverWorkspaceSection: "overview",
    steamCmdStatus: null
  };

  const assistant = buildAssistantViewModel(input);
  assert.deepEqual(assistant.issues, []);
  assert.deepEqual(assistant.prompts, []);
  assert.match(assistant.contextPayload, /Storage Ready: false/);
  assert.doesNotMatch(assistant.contextPayload, /Module Count|Instance Count|Running Instances|Instance Roster/);
  assert.equal(buildAssistantCapsuleModel(input).tone, "info");
});

test("desktop routes, counts, and storage polling stay gated until atomic initialization succeeds", () => {
  const appSource = fs.readFileSync(path.join(desktopRoot, "src", "App.tsx"), "utf8");
  const mainSource = fs.readFileSync(path.join(desktopRoot, "src", "main.tsx"), "utf8");
  const headerSource = fs.readFileSync(path.join(desktopRoot, "src", "components", "AppHeader.tsx"), "utf8");
  const hookSource = fs.readFileSync(path.join(desktopRoot, "src", "hooks", "useDesktopEffects.ts"), "utf8");
  const coordinatorSource = fs.readFileSync(
    path.join(desktopRoot, "src", "bootstrap-initialization.ts"),
    "utf8"
  );
  const routerSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "AppViewRouter.tsx"), "utf8");
  const assistantPanelSource = fs.readFileSync(path.join(desktopRoot, "src", "components", "AssistantPanel.tsx"), "utf8");
  const storageViewSource = fs.readFileSync(path.join(desktopRoot, "src", "components", "StorageInitializationView.tsx"), "utf8");
  const storageStyleSource = fs.readFileSync(path.join(desktopRoot, "src", "styles", "core-pages", "storage-initialization.css"), "utf8");

  assert.match(appSource, /storageInitializationSurface === "content"\s*\?\s*\(\s*<AppViewRouter/);
  assert.match(appSource, /serverCount=\{storageReady \? instances\.length : null\}/);
  assert.match(appSource, /const shouldPollSystemView = activeView === "system" && storageReady;/);
  let pollingEnabled;
  visitSyntax(parseSource(appSource, "App.tsx"), (node) => {
    if (node.type !== "CallExpression" || node.callee?.value !== "useLibraryJobPolling") return;
    const property = node.arguments[0]?.expression?.properties?.find((entry) => entry.key?.value === "enabled");
    assert.ok(property?.value, "library polling must have an explicit admission condition");
    pollingEnabled = sourceText(appSource, property.value);
  });
  assert.ok(pollingEnabled, "library job polling must remain registered");
  for (const storageReady of [false, true]) {
    for (const activeView of ["library", "servers", "system"]) {
      for (const libraryTaskPolling of [false, true]) {
        for (const hasActiveJobs of [false, true]) {
          for (const hasCreatingModules of [false, true]) {
            const state = { storageReady, activeView, libraryTaskPolling, hasActiveJobs,
              creatingModuleIds: new Set(hasCreatingModules ? ["minecraft"] : []) };
            const expected = storageReady && (activeView !== "system" || libraryTaskPolling || hasActiveJobs || hasCreatingModules);
            assert.equal(vm.runInNewContext(pollingEnabled, state), expected,
              JSON.stringify({ ...state, creatingModuleIds: [...state.creatingModuleIds] }));
          }
        }
      }
    }
  }
  assert.match(headerSource, /item\.key === "servers" && props\.serverCount !== null/);
  assert.doesNotMatch(routerSource, /libraryInitializationError|initializationError/);
  assert.match(assistantPanelSource, /props\.assistantInput\.storageReady \? <div className="assistant-chat-composer">/);
  assert.match(storageViewSource, /className="storage-initialization-retry-button"/);
  assert.match(storageViewSource, /storage\.initialization\.failedBody/);
  assert.match(storageViewSource, /<details/);
  assert.match(storageViewSource, /props\.state\.error/);
  assert.doesNotMatch(storageViewSource, /databasePath|logDirectory/);
  assert.doesNotMatch(storageStyleSource, /storage-initialization-(card|safety|icon)/);
  assert.match(mainSource, /<React\.StrictMode>/);
  assert.match(hookSource, /bootstrapInitializationCoordinator\.request\(attempt\)/);
  assert.match(hookSource, /void flight\.metadata\.then/);
  assert.match(hookSource, /void flight\.initialization\.then/);
  assert.match(hookSource, /return \(\) => \{\s*cancelled = true;/);

  const bootstrapIndex = coordinatorSource.indexOf("await services.bootstrapApp()");
  const ensureIndex = coordinatorSource.indexOf("await services.ensureStorageReady()");
  const syncIndex = coordinatorSource.indexOf("await services.syncModulesToStorage()");
  const listIndex = coordinatorSource.indexOf("await services.listInstancesFromStorage()");
  assert.ok(bootstrapIndex >= 0 && bootstrapIndex < ensureIndex);
  assert.ok(syncIndex < listIndex);
  assert.ok(ensureIndex < syncIndex);
  assert.match(coordinatorSource, /const existing = flights\.get\(attempt\);/);
  assert.match(coordinatorSource, /if \(flights\.get\(attempt\) === flight\) \{\s*flights\.delete\(attempt\);/);
  assert.match(hookSource, /\}, \[options\.attempt\]\);/);

  assert.match(
    coordinatorSource,
    /initialization: initialize\(attempt\),\s*metadata: loadMetadata\(attempt\)/
  );
  const metadataHandler = appSource.slice(
    appSource.indexOf("const handleBootstrapMetadata"),
    appSource.indexOf("const handleBootstrapInitFailed")
  );
  assert.match(metadataHandler, /storageInitialization\.attempt !== payload\.attempt/);
  assert.doesNotMatch(metadataHandler, /storageInitialization\.status/);
});

test("runtime heartbeat only reconciles an explicitly ready storage state and never initializes storage itself", () => {
  const stateSource = fs.readFileSync(path.join(workspaceRoot, "apps", "desktop", "src-tauri", "src", "state.rs"), "utf8");
  const commandsSource = fs.readFileSync(path.join(workspaceRoot, "apps", "desktop", "src-tauri", "src", "commands.rs"), "utf8");
  const reconcileSource = fs.readFileSync(
    path.join(workspaceRoot, "apps", "desktop", "src-tauri", "src", "commands_runtime_supervision.rs"),
    "utf8"
  );
  const storageCommandsSource = fs.readFileSync(
    path.join(workspaceRoot, "apps", "desktop", "src-tauri", "src", "commands_storage.rs"),
    "utf8"
  );

  assert.match(stateSource, /app_state\.storage = default_paths\.probe_status\(\)/);
  assert.match(commandsSource, /if !state\.is_storage_ready\(\) \{\s*continue;\s*\}[\s\S]*reconcile_runtime_state/);

  const reconcileFunction = reconcileSource.slice(
    reconcileSource.indexOf("pub(super) async fn reconcile_runtime_state"),
    reconcileSource.indexOf("fn log_runtime_performance_refreshes")
  );
  assert.doesNotMatch(reconcileFunction, /initialize_database/);

  const initializeIndex = storageCommandsSource.indexOf("let status = initialize_database");
  const publishReadyIndex = storageCommandsSource.indexOf("state_guard.storage = status.clone()");
  assert.ok(initializeIndex >= 0 && initializeIndex < publishReadyIndex);
});
