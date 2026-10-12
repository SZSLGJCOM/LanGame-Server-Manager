const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadApi(invoke) {
  function load(name) {
    const filename = path.join(__dirname, "../src", `${name}.ts`);
    const exports = {};
    vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename, { development: false }), {
      exports,
      require(id) {
        if (id === "@tauri-apps/api/core") return { isTauri: () => true, invoke };
        if (id === "./desktop-exit-lifecycle") return require("./helpers/desktop-exit-fixture.cjs")
          .loadDesktopExitModule({ isTauri: () => true, invoke });
        if (["./api-transport", "./storage-management-requests", "./i18n-config", "./ark-map-availability", "./views/servers/mod-workbench-asa"].includes(id)) return load(id.slice(2));
        if (id === "./locale-preference") return { readPreferredLocale: () => "en-US" };
        throw new Error(`Unexpected dependency: ${id}`);
      }
    }, { filename });
    return exports;
  }
  return load("api");
}

test("creation delegates clean program preparation to the backend without a source override", async () => {
  const requests = [];
  const provisioning = { summary: { id: "created" } };
  const api = loadApi(async (command, args) => {
    requests.push(JSON.parse(JSON.stringify({ command, args })));
    return provisioning;
  });
  assert.equal(await api.createInstance({ name: "Default", module_id: "minecraft" }), provisioning);
  await api.createInstance({ name: "Independent", module_id: "minecraft", program_mode: "independent" });
  assert.deepEqual(requests, [
    { command: "create_instance_record", args: { input: { name: "Default", module_id: "minecraft" } } },
    { command: "create_instance_record", args: { input: { name: "Independent", module_id: "minecraft" }, programMode: "independent" } }
  ]);
});

test("program preparation failures retain their native cause", async () => {
  const failure = new Error("No verified local program is available; install or verify the library program.");
  const api = loadApi(async () => { throw failure; });
  await assert.rejects(api.createInstance({ name: "Missing", module_id: "minecraft" }), (error) => error === failure);
});

test("command arguments reach the native transport in their declared shape", async () => {
  const requests = [];
  const api = loadApi(async (command, args) => {
    requests.push(JSON.parse(JSON.stringify({ command, args })));
    return { state: { snapshot: { telemetry: {} } } };
  });
  const cases = [
    [() => api.bootstrapApp({ includeSystemSnapshot: true }), "bootstrap", { includeSystemSnapshot: true }],
    [() => api.pickDirectoryPath("fixture-path"), "pick_directory_path", { currentPath: "fixture-path" }],
    [() => api.readInstanceConnectionInfo(["fixture"]), "read_instance_connection_info_from_storage", { instanceIds: ["fixture"] }],
    [() => api.readInstanceLivePlayers("fixture"), "read_instance_live_players", { instanceId: "fixture" }],
    [() => api.refreshInstanceLivePlayers("fixture"), "refresh_instance_live_players", { instanceId: "fixture" }],
    [() => api.executeInstancePlayerAction({ instance_id: "fixture", snapshot_id: "snapshot", player_key: "player", action_id: "kick" }), "execute_instance_player_action", { input: { instance_id: "fixture", snapshot_id: "snapshot", player_key: "player", action_id: "kick" } }],
    [() => api.renameInstanceBackup("fixture", "backup", null), "rename_instance_backup", { instanceId: "fixture", backupId: "backup", displayName: null }],
    [() => api.downloadSteamWorkshopItems("fixture", ["123"], true), "download_steam_workshop_items", { instanceId: "fixture", ids: ["123"], missingOnly: true, locale: "en-US" }],
    [() => api.stageManualModFiles("fixture", ["mod.zip"]), "stage_manual_mod_files", { instanceId: "fixture", sourcePaths: ["mod.zip"] }],
    [() => api.importDontStarveWorldData("fixture", "world.zip"), "import_dontstarve_world_data", { instanceId: "fixture", sourcePath: "world.zip", locale: "en-US" }],
    [() => api.searchSteamWorkshopItems(123, "mod", "trend", 2, "en-US", "collection"), "search_steam_workshop_items", { appId: 123, query: "mod", sort: "trend", page: 2, locale: "en-US", browseKind: "collection" }],
    [() => api.startInstance("fixture", { instance_id: "fixture", settings_json: "{}", shards: [{ shard: "Master", state: "existing", enabled: true }] }), "start_instance_process", { instanceId: "fixture", expectedWorldStart: { instance_id: "fixture", settings_json: "{}", shards: [{ shard: "Master", state: "existing", enabled: true }] } }],
    [() => api.listOllamaModels("http://127.0.0.1:11434"), "assistant_list_ollama_models", { baseUrl: "http://127.0.0.1:11434" }],
    [() => api.readInstanceIsolation("fixture"), "read_instance_isolation", { input: { instance_id: "fixture" } }],
    [() => api.executeDeclaredRuntimePlayerAction("fixture", "ban", "player", ""), "execute_instance_manual_player_action", { input: { instance_id: "fixture", action_id: "ban", target: "player", role: null } }]
  ];
  for (const [call, command, args] of cases) {
    const before = requests.length;
    await call();
    assert.equal(requests.length, before + 1, command);
    assert.deepEqual(requests.at(-1), { command, args });
  }
  for (const [call, command, input] of [
    [api.generateInstanceBroadcast, "generate_instance_broadcast", { instanceId: "fixture", settings: {}, intent: "hello", ruleId: null, policySnapshotJson: "{}" }],
    [api.sendInstanceBroadcast, "send_instance_broadcast", { instanceId: "fixture", message: "hello", ruleId: null, aiProvider: "local", aiModel: "fixture-model", policySnapshotJson: "{}" }]
  ]) {
    await call(input);
    assert.deepEqual(requests.at(-1), { command, args: { input } });
  }
});
