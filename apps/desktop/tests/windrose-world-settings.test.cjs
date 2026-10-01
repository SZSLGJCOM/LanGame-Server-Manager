const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repositoryRoot = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(repositoryRoot, "apps", "desktop");
const moduleRoot = path.join(repositoryRoot, "modules", "windrose");
const read = (...segments) => fs.readFileSync(path.join(...segments), "utf8");

const schema = JSON.parse(read(moduleRoot, "schema.json"));
const definitionSource = read(desktopRoot, "src", "views", "settings", "modules", "windrose.ts");
const panelSource = read(desktopRoot, "src", "views", "settings", "WindroseWorldSettingsPanel.tsx");
const storageRoot = path.join(repositoryRoot, "crates", "app-storage", "src");
const materializerSource = [
  "windrose.rs",
  "windrose_document.rs",
  "windrose_plan.rs",
  "windrose_updater.rs"
].map((fileName) => read(storageRoot, "templates_materialize", fileName)).join("\n");
const updaterSource = read(storageRoot, "templates_materialize", "windrose_updater.rs");
const instancesSource = read(storageRoot, "instances.rs");
const runtimeLifecycleSource = read(
  desktopRoot,
  "src-tauri",
  "src",
  "commands_runtime_lifecycle.rs"
);
const englishMessages = read(desktopRoot, "src", "i18n", "games", "windrose.en.ts");
const chineseMessages = read(desktopRoot, "src", "i18n", "games", "windrose.zh-cn.ts");

const worldFields = [
  "world_name",
  "world_preset_type",
  "coop_quests",
  "easy_explore",
  "mob_health_multiplier",
  "mob_damage_multiplier",
  "ship_health_multiplier",
  "ship_damage_multiplier",
  "boarding_difficulty_multiplier",
  "coop_stats_correction_modifier",
  "coop_ship_stats_correction_modifier",
  "combat_difficulty"
];

test("Windrose keeps world identity in Room and per-world rules in their specialized Configuration editor", () => {
  for (const fieldKey of worldFields) {
    assert.ok(schema.properties[fieldKey], fieldKey);
    assert.equal(schema.properties[fieldKey]["x-lsgm-section"], fieldKey === "world_name" ? "room" : "world", fieldKey);
    assert.match(definitionSource, new RegExp(`(?:"${fieldKey}"|\\b${fieldKey}\\s*:)`), fieldKey);
  }
  assert.match(definitionSource, /"windrose-world-settings"\s*:\s*\{/);
  assert.match(definitionSource, /"windrose-world-name"\s*:\s*\{/);
  assert.match(definitionSource, /Renderer:\s*WindroseWorldSettingsPanel/);
  assert.match(definitionSource, /initializeSettings:\s*\(settings\)\s*=>\s*\(\{\s*\.\.\.WINDROSE_WORLD_DEFAULTS,\s*\.\.\.settings\s*\}\)/);
});

test("world controls remain visibly disabled until an existing world ID is selected", () => {
  assert.match(panelSource, /selectedWorldId\.length === 0/);
  assert.match(panelSource, /data-windrose-world-state="selection-required"/);
  assert.match(panelSource, /disabled=\{disabled\}/);
  assert.match(panelSource, /settings remain unavailable until one world can be identified/i);
  assert.doesNotMatch(panelSource, /exact native world|native match/i);
  assert.match(panelSource, /buildConfigurationFieldIds\(fieldKey, "configuration-windrose"\)\.inputId/);
});

test("world fields expose concise hover help without visible native implementation evidence", () => {
  assert.match(panelSource, /useConfigurationFieldHelp/u);
  assert.match(panelSource, /data-windrose-native-key/u);
  assert.match(panelSource, /aria-describedby=\{descriptionId\}/u);
  assert.doesNotMatch(panelSource, /configuration-field-evidence/u);
  assert.doesNotMatch(panelSource, /nativeKeyLabel/u);
  assert.doesNotMatch(panelSource, /<p className="form-note" id=/u);
});

test("world controls require a fully stopped server and explain when changes take effect", () => {
  assert.match(panelSource, /props\.details\.summary\.status/);
  assert.match(panelSource, /\["starting", "running", "stopping"\]/);
  assert.match(panelSource, /serverMustStop/);
  assert.match(panelSource, /data-windrose-world-state="server-stop-required"/);
  assert.match(englishMessages, /Saved changes take effect on the next start/i);
  assert.doesNotMatch(englishMessages, /official Windrose updater/i);
  assert.match(chineseMessages, /保存后的改动会在下次启动时生效/);
  assert.doesNotMatch(chineseMessages, /官方 Updater/);
});

test("storage resolves an exact identity and uses compare-and-swap replacement", () => {
  for (const marker of [
    "EmptySelection",
    "UnsafeSelection",
    "NoMatch",
    "Ambiguous",
    "WorldDescription islandId",
    "ServerDescription WorldIslandId",
    "OutsideInstallRoot",
    "ConcurrentModification",
    "RollbackFailed"
  ]) {
    assert.match(materializerSource, new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")), marker);
  }
  assert.match(materializerSource, /compare_and_swap_file_atomically/);
  assert.match(materializerSource, /merge_json\(&mut merged_server/);
});

test("official multiplier ranges are represented without guessed bounds", () => {
  const expected = {
    mob_health_multiplier: [0.2, 5],
    mob_damage_multiplier: [0.2, 5],
    ship_health_multiplier: [0.4, 5],
    ship_damage_multiplier: [0.2, 2.5],
    boarding_difficulty_multiplier: [0.2, 5],
    coop_stats_correction_modifier: [0, 2],
    coop_ship_stats_correction_modifier: [0, 2]
  };
  for (const [fieldKey, [minimum, maximum]] of Object.entries(expected)) {
    assert.equal(schema.properties[fieldKey].minimum, minimum, fieldKey);
    assert.equal(schema.properties[fieldKey].maximum, maximum, fieldKey);
  }
  assert.deepEqual(schema.properties.world_preset_type.enum, ["Easy", "Medium", "Hard"]);
  assert.deepEqual(schema.properties.combat_difficulty.enum, ["Easy", "Normal", "Hard"]);
});

test("save stages world changes and only the startup path runs the bounded updater after commit", () => {
  assert.match(updaterSource, /R5WorldDescriptionUpdater\.exe/);
  assert.match(updaterSource, /current_dir\(&invocation\.working_directory\)/);
  assert.match(updaterSource, /\.arg\(&invocation\.argument\)/);
  assert.match(updaterSource, /kill_on_drop\(true\)/);
  assert.match(updaterSource, /tokio::time::timeout/);
  assert.match(updaterSource, /start_kill\(\)/);
  assert.match(updaterSource, /child\.wait\(\)/);
  assert.match(updaterSource, /TerminationUnconfirmed/);
  assert.match(updaterSource, /creation_flags\(windrose_updater_creation_flags\(\)\)/);
  assert.match(runtimeLifecycleSource, /materialize_instance_configuration_for_start/);
  const startupStart = runtimeLifecycleSource.indexOf("start_instance_process_after_reconcile_reserved");
  assert.notEqual(startupStart, -1);
  const startupSource = runtimeLifecycleSource.slice(startupStart);
  const materialization = startupSource.indexOf("materialize_runtime_start_configuration(");
  const prestartUpdate = startupSource.indexOf("run_prestart_update_if_needed(");
  const processSpawn = startupSource.indexOf("spawn_launch_plan_in_resource_group(");
  assert.ok(
    prestartUpdate >= 0 && prestartUpdate < materialization,
    "program updates must finish before rendering settings and applying committed world changes"
  );
  assert.ok(
    materialization >= 0 && materialization < processSpawn,
    "a failed updater must return before the Windrose server can be spawned"
  );
  const materializationHelperStart = runtimeLifecycleSource.indexOf(
    "async fn materialize_runtime_start_configuration"
  );
  assert.notEqual(materializationHelperStart, -1);
  const materializationHelperSource = runtimeLifecycleSource.slice(materializationHelperStart);
  assert.match(materializationHelperSource, /materialize_instance_configuration_for_start\(/);

  const policyStart = instancesSource.indexOf("async fn materialize_instance_configuration_with_policy");
  assert.notEqual(policyStart, -1);
  const policySource = instancesSource.slice(policyStart);
  const coordinatedWrite = policySource.indexOf("write_pending_instance_configuration_in_worker(");
  const commit = policySource.indexOf("commit_instance_transaction(tx, config_mutation, settings_lock).await?");
  const updater = policySource.indexOf("apply_module_prestart_support(&prestart_context, settings_lock).await?");
  assert.ok(coordinatedWrite >= 0 && coordinatedWrite < commit);
  assert.ok(commit < updater, "external updater must run only after the database transaction commits");
  assert.match(policySource, /load_active_instance_run\(&mut \*tx, instance_id\)/);
  assert.match(policySource, /instance_running:\s*instance_running \|\| active_run\.is_some\(\)/);
  assert.match(instancesSource, /materialize_instance_configuration_with_policy\(paths, instance_id, settings_lock, false\)/);
  assert.match(instancesSource, /materialize_instance_configuration_with_policy\(paths, instance_id, &settings_lock, true\)/);
});
