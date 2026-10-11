const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (loaded, filename) => {
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".css"] = (loaded) => loaded._compile("module.exports = {};", loaded.filename);
const { ARK_ADDITIONAL_MAP_LIMIT, readAdditionalArkMaps, validateAdditionalArkMaps, readArkMapSuggestions,
  buildArkMapSelectionPatch, mergeArkMapSuggestions, newArkMapId, getArkMapSettingsValidationIssues, buildArkMapPortGroups } = require("../src/views/settings/ark-cluster-maps.ts");
const { parseInstanceArkMapAvailability } = require("../src/ark-map-availability.ts");
const map = (overrides = {}) => ({ id: "scorched", map_name: "ScorchedEarth_WP", name: "焦土", enabled: true, ...overrides });

test("maps preserve distinct save identities and paused configurations across JSON readback", () => {
  const settings = JSON.parse(JSON.stringify({ additional_maps: [map(), map({ id: "center", map_name: "TheCenter_WP", enabled: false })] }));
  assert.equal(validateAdditionalArkMaps(settings), null);
  assert.deepEqual(readAdditionalArkMaps(settings), settings.additional_maps);
  assert.deepEqual(readAdditionalArkMaps({}), []);
});

test("the map boundary rejects unsafe packages, duplicate identities and malformed topology", () => {
  const cases = [
    [{ additional_maps: null }, "shape"], [{ additional_maps: [{}] }, "shape"],
    [{ additional_maps: [map({ enabled: "true" })] }, "shape"],
    [{ additional_maps: [map({ unexpected: "preserve rather than silently discard" })] }, "shape"],
    [{ additional_maps: [map({ id: "../outside" })] }, "id"],
    [{ additional_maps: [map(), map()] }, "duplicate"],
    [{ additional_maps: [map({ map_name: "TheIsland?Port=1" })] }, "map"],
    [{ additional_maps: [map({ map_name: "../TheIsland" })] }, "map"],
    [{ additional_maps: [map({ map_name: "a".repeat(129) })] }, "map"],
    [{ additional_maps: [map({ name: "\u0085" })] }, "name"],
    [{ additional_maps: [map({ name: "Bad?Name" })] }, "name"],
    [{ additional_maps: [map({ name: "Bad\u2028Name" })] }, "name"],
    [{ additional_maps: [map({ name: " " })] }, "name"],
    [{ additional_maps: Array.from({ length: ARK_ADDITIONAL_MAP_LIMIT + 1 }, (_, index) => map({ id: `world-${index}` })) }, "limit"]
  ];
  for (const [settings, reason] of cases) {
    assert.equal(validateAdditionalArkMaps(settings), reason);
    assert.equal(getArkMapSettingsValidationIssues(settings, (key) => key)[0].fieldKey, "additional_maps");
  }
  assert.equal(validateAdditionalArkMaps({ additional_maps: [map({ name: "🦖".repeat(80) })] }), null);
  assert.equal(validateAdditionalArkMaps({ additional_maps: [map({ name: "Bob's map" })] }), null);
});

test("complete server names retain the primary schema's unbounded length and their own identities", () => {
  const name = "Independent full server name 🦖 ".repeat(8).trim();
  for (const edition of ["arksurvivalascended", "arksurvivalevolved"]) {
    const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, `../../../modules/${edition}/schema.json`), "utf8"));
    assert.equal(schema.properties.server_name.maxLength, undefined);
    const settings = { server_name: "A different primary server", additional_maps: [map({ name })] };
    assert.equal(validateAdditionalArkMaps(settings), null);
    assert.deepEqual(readAdditionalArkMaps(JSON.parse(JSON.stringify(settings))), [map({ name })]);
    assert.equal(settings.server_name, "A different primary server");
  }
});

test("converted complete server names retain legal surrounding spaces verbatim", () => {
  for (const name of ["Leading space ", "  Legacy primary | 焦土  ", "\u3000Existing full server\u3000"]) {
    const settings = { server_name: "  Legacy primary  ", additional_maps: [map({ name })] };
    assert.equal(validateAdditionalArkMaps(settings), null);
    assert.deepEqual(readAdditionalArkMaps(JSON.parse(JSON.stringify(settings))), [map({ name })]);
    assert.equal(settings.server_name, "  Legacy primary  ");
  }
});

test("quoted complete server names roundtrip and render as escaped input text", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { I18nContext } = require("../src/i18n-context.ts");
  const { ArkClusterMapsEditor } = require("../src/views/settings/ArkClusterMapsEditor.tsx");
  const name = '\"><img src=x onerror="globalThis.arkNameScriptRan=1"><script>globalThis.arkNameScriptRan=1</script>';
  for (const quoted of ['Bad"Name', '"Legacy primary" | 焦土', name]) {
    const settings = { server_name: quoted, map_name: "TheIsland_WP", additional_maps: [map({ name: quoted })] };
    assert.equal(validateAdditionalArkMaps(settings), null);
    assert.deepEqual(readAdditionalArkMaps(JSON.parse(JSON.stringify(settings))), [map({ name: quoted })]);
  }
  const settings = { server_name: name, map_name: "TheIsland_WP", additional_maps: [map({ name })] };
  const html = renderToStaticMarkup(React.createElement(I18nContext.Provider,
    { value: { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key } },
    React.createElement(ArkClusterMapsEditor, { sectionId: "transfer", settings, disabled: false, onPatch() {},
      moduleDetails: { schema_json: "{}" },
      details: { summary: { id: "owned", name: "Unrelated instance", status: "Stopped", active_process_count: 0 },
        settings_json: JSON.stringify(settings), ports: [], active_run: null } })));
  assert.match(html, /value="&quot;&gt;&lt;img/);
  assert.doesNotMatch(html, /<(?:img|script)\b|\s(?:onerror|onfocus)=["']/);
  assert.deepEqual(settings.additional_maps, [map({ name })]);
});

test("the map picker reads each edition's native suggestions and keeps custom packages available", () => {
  for (const [edition, primary] of [["arksurvivalascended", "TheIsland_WP"], ["arksurvivalevolved", "TheIsland"]]) {
    const source = fs.readFileSync(path.resolve(__dirname, `../../../modules/${edition}/schema.json`), "utf8");
    const chinese = readArkMapSuggestions(source, "zh-CN");
    assert.ok(chinese.some((entry) => entry.value === primary && entry.label.includes("孤岛")));
    assert.ok(chinese.some((entry) => /Ragnarok/.test(entry.value)));
    assert.equal(new Set(chinese.map((entry) => entry.value)).size, chinese.length);
    assert.equal(readArkMapSuggestions(source, "en-US").find((entry) => entry.value === primary).label, "The Island");
  }
});

test("ASA defaults offer official maps without advertising undownloaded mod maps", () => {
  const source = fs.readFileSync(path.resolve(__dirname, "../../../modules/arksurvivalascended/schema.json"), "utf8");
  const values = readArkMapSuggestions(source, "en-US").map((entry) => entry.value);
  for (const official of ["TheIsland_WP", "ScorchedEarth_WP", "TheCenter_WP", "Aberration_WP", "Extinction_WP",
    "Astraeos_WP", "Ragnarok_WP", "Valguero_WP", "Genesis_WP", "LostColony_WP"]) {
    assert.ok(values.includes(official), `Official map ${official} must remain available`);
  }
  for (const notDownloaded of ["Atlantis_WP", "Forglar_WP", "Svartalfheim_WP", "Appalachia_Official_WP", "Amissa_WP", "BobsMissions_WP"])
    assert.ok(!values.includes(notDownloaded), `Mod map ${notDownloaded} requires actual installed content`);
});

test("installed native packages augment official choices without guessing, duplication or mutating a saved map", () => {
  const official = [{ value: "TheIsland_WP", label: "The Island" }];
  const installed = [{ value: "NewCommunityMap_WP", label: "Downloaded map" },
    { value: "TheIsland_WP", label: "A mod must not rename the official map", modId: "893657" },
    { value: "NewCommunityMap_WP", label: "A second package" }, { value: "../unsafe", label: "Unsafe path" }];
  const retained = { map_name: "RemovedMap_WP", additional_maps: [map({ map_name: "AnotherRemovedMap_WP" })] };
  const before = JSON.stringify(retained);
  const choices = mergeArkMapSuggestions(official, installed);
  assert.deepEqual(choices, [...official, installed[0]]);
  assert.deepEqual(mergeArkMapSuggestions(official, []), official);
  assert.equal(JSON.stringify(retained), before);
  assert.equal(choices.some((choice) => choice.value === retained.map_name), false);
  assert.deepEqual(buildArkMapSelectionPatch(retained, "TheIsland_WP", choices, { map_name: "TheIsland_WP" }), { map_name: "TheIsland_WP" });
});

test("selecting either map control atomically enables its discovered Mod, preserves order and removes passive conflicts", () => {
  const choices = [{ value: "TheIsland_WP", label: "The Island" }, { value: "ActualDownloaded_WP", label: "Mod", modId: "893657" }];
  for (const settings of [
    { mod_ids_csv: "100,200", passive_mod_ids_csv: "300,893657,400", curseforge_disabled_mod_ids: ["893657", "500"] },
    { mod_ids_csv: "100,893657,200", passive_mod_ids_csv: "300,893657,400", curseforge_removed_mod_ids: ["893657", "600"] },
    { mod_ids_csv: "100,200", curseforge_removed_mod_ids: ["893657", "600"] }
  ]) {
    for (const mapPatch of [{ map_name: "ActualDownloaded_WP" }, { additional_maps: [map({ map_name: "ActualDownloaded_WP" })] }]) {
      const current = { server_name: "Retain this name", ...settings };
      const before = structuredClone(current);
      const patch = buildArkMapSelectionPatch(current, "ActualDownloaded_WP", choices, mapPatch);
      const next = { ...current, ...patch };
      assert.deepEqual(current, before);
      assert.deepEqual(patch.map_name ?? patch.additional_maps, mapPatch.map_name ?? mapPatch.additional_maps);
      assert.equal(next.mod_ids_csv, settings.mod_ids_csv.includes("893657") ? "100,893657,200" : "100\n200\n893657");
      if ("passive_mod_ids_csv" in settings) assert.equal(next.passive_mod_ids_csv, "300\n400");
      assert.deepEqual(next.curseforge_disabled_mod_ids ?? [], settings.curseforge_disabled_mod_ids ? ["500"] : []);
      assert.deepEqual(next.curseforge_removed_mod_ids ?? [], settings.curseforge_removed_mod_ids ? ["600"] : []);
      assert.equal(next.server_name, current.server_name);
    }
  }
});

test("official or unknown choices leave membership alone; raw Mod flags reject a known Mod selection without a partial patch", () => {
  const current = { custom_launch_flags: "-mods=100", mod_ids_csv: "200", map_name: "TheIsland_WP" };
  const choices = [{ value: "ActualDownloaded_WP", label: "Downloaded", modId: "893657" }];
  const before = structuredClone(current);
  assert.throws(() => buildArkMapSelectionPatch(current, "ActualDownloaded_WP", choices, { map_name: "ActualDownloaded_WP" }),
    (error) => error.code === "raw-mod-flags");
  assert.deepEqual(current, before);
  assert.deepEqual(buildArkMapSelectionPatch(current, "TheIsland_WP", choices, { map_name: "TheIsland_WP" }), { map_name: "TheIsland_WP" });
  assert.deepEqual(buildArkMapSelectionPatch(current, "NotDownloaded_WP", choices, { map_name: "NotDownloaded_WP" }), { map_name: "NotDownloaded_WP" });
});

test("availability accepts only this ASA instance's bounded actual package metadata", () => {
  const installed = { map_name: "NewAuthorPackage_WP", mod_id: "893657", label: "Author title that is not a map identifier" };
  const response = { instance_id: "mine", module_id: "arksurvivalascended", maps: [installed] };
  assert.deepEqual(parseInstanceArkMapAvailability(response, "mine"), response);
  assert.deepEqual(parseInstanceArkMapAvailability({ ...response, maps: [] }, "mine").maps, []);
  for (const invalid of [{ ...response, instance_id: "other" }, { ...response, module_id: "arksurvivalevolved" },
    { ...response, maps: [{ ...installed, map_name: "../Other_WP" }] }, { ...response, maps: [{ ...installed, mod_id: "0" }] },
    { ...response, maps: [{ ...installed, label: "a".repeat(257) }] }, { ...response, maps: Array(8193).fill(installed) }])
    assert.throws(() => parseInstanceArkMapAvailability(invalid, "mine"), /Invalid installed ARK map availability/);
  assert.equal(parseInstanceArkMapAvailability({ ...response, maps: [{ ...installed, label: "🦖".repeat(256) }] }, "mine").maps[0].label.length, 512);
});

test("new maps receive fresh safe identities with bounded collision handling", () => {
  const existing = newArkMapId([], () => "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
  let calls = 0;
  const next = newArkMapId([existing], () => ++calls === 1 ? "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa" : "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
  assert.notEqual(next, existing);
  assert.match(next, /^[a-z0-9][a-z0-9-]{0,31}$/);
  assert.equal(calls, 2);
  let collisions = 0;
  assert.throws(() => newArkMapId([existing], () => { collisions++; return "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"; }), /unique ARK map identity/);
  assert.equal(collisions, 10);
});

test("ASE map port groups keep game and peer together including paused maps", () => {
  const base = [{ id: "game_peer", members: ["game", "peer"], member_offsets: { game: 0, peer: 1 } }];
  const settingsJson = JSON.stringify({ additional_maps: [map(), map({ id: "center", enabled: false })] });
  const groups = buildArkMapPortGroups("arksurvivalevolved", settingsJson, base);
  assert.equal(groups.length, 3);
  assert.deepEqual(groups[1], { id: "map-scorched-game_peer", members: ["map-scorched-game", "map-scorched-peer"], member_offsets: { "map-scorched-game": 0, "map-scorched-peer": 1 } });
  assert.deepEqual(groups[2].members, ["map-center-game", "map-center-peer"]);
  assert.equal(buildArkMapPortGroups("terraria", settingsJson, base), base);
  assert.equal(buildArkMapPortGroups("arksurvivalevolved", "bad json", base), base);
  assert.deepEqual(buildArkMapPortGroups("arksurvivalascended", settingsJson, []), []);
});
