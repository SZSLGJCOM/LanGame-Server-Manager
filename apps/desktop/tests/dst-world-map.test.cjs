const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { parseDstWorldMap, mapPlayerArtwork, mapPlayerPoint, terrainColor } = require("../src/dst-world-map.ts");
const { dstLandmarkIconSize, indexDstLandmarks, visibleDstLandmarks } = require("../src/dst-map-landmarks.ts");
const { clampDstMapZoom, stepDstMapZoom, anchoredDstMapCenter } = require("../src/dst-map-view.ts");
const { dstMapEdgeMask, DST_MAP_EDGE_MASKS } = require("../src/dst-map-edge-shapes.ts");
const valid = () => ({ instanceId: "a", shard: "volcano", runId: 2, capturedAtUnixMs: 123456,
  session: "world-a", width: 2, height: 2, tileScale: 4, day: 3, season: "summer", phase: "day",
  tiles: [1, 3, 3, 1], tileNames: { 1: "IMPASSABLE", 3: "MOD_VOLCANO" }, tileKinds: { 1: "void", 3: "land" },
  tileTextures: {}, tileBaseTextures: {}, tileColors: {}, terrainLayers: [], landmarks: [], roads: [], landmarksTruncated: false,
  art: { textures: {}, icons: {}, iconFallbacks: [], issues: [], terrainBaseColor: null, terrainBaseColors: {}, terrainEdges: [] },
  players: [{ id: "KU_1", name: "A", character: "wilson", icon: "wilson.png", x: -1, z: 1, mapX: .75, mapZ: 1.25 }] });

test("native terrain, mod names and fractional player positions survive decoding", () => {
  const value = valid();
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true), value);
  assert.deepEqual(mapPlayerPoint(value.players[0], 2), [.75, .75]);
  assert.notEqual(terrainColor("MOD_VOLCANO"), terrainColor("OCEAN_DEEP"));
});

test("native terrain layers preserve duplicate order and a broken final material", () => {
  const value = valid();
  value.terrainLayers = [
    { tileId: 3, atlas: "first.xml", texture: "first.tex", noiseTexture: "shared.tex", sampleStyle: 0 },
    { tileId: 3, atlas: null, texture: null, noiseTexture: null, sampleStyle: null },
  ];
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true).terrainLayers, value.terrainLayers);
  for (const layer of [{ ...value.terrainLayers[0], tileId: 65536 }, { ...value.terrainLayers[0], atlas: "bad\npath" },
    { ...value.terrainLayers[0], noiseTexture: undefined }, { ...value.terrainLayers[0], sampleStyle: -1 }]) {
    assert.throws(() => parseDstWorldMap({ ...value, terrainLayers: [layer] }, "a", "volcano", true));
  }
});

test("terrain edge artwork retains atlas and explicit texture identity with bounded complete shapes", () => {
  const source = { atlas: "one.xml", texture: "explicit.tex", image: "data:image/png;base64,AA==", opaque: [0, 0, .1, .1],
    samples: Object.fromEntries([...DST_MAP_EDGE_MASKS].map(mask => [String(mask), [.1, .1, .1, .1]])) };
  const value = valid();
  value.art.terrainEdges = [source, { ...source, atlas: "two.xml" }, { ...source, texture: "other.tex" }];
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true).art.terrainEdges, value.art.terrainEdges);
  const missing = { ...source.samples }; delete missing[0];
  for (const terrainEdges of [[source, source], [{ ...source, samples: missing }],
    [{ ...source, samples: { ...missing, "01": [.1, .1, .1, .1] } }],
    [{ ...source, opaque: [.9, 0, .2, .1] }], [{ ...source, image: "https://example.test/private" }]]) {
    assert.throws(() => parseDstWorldMap({ ...value, art: { ...value.art, terrainEdges } }, "a", "volcano", true));
  }
  assert.equal(DST_MAP_EDGE_MASKS.size, 47);
  assert.equal(dstMapEdgeMask(1), 1, "A lone diagonal must stay independent");
  assert.equal(dstMapEdgeMask(2), 7, "A north neighbor must cover both northern corners");
  assert.equal(dstMapEdgeMask(2 | 8), 47, "Adjacent cardinal directions must absorb their exterior corners");
  assert.equal(dstMapEdgeMask(2 | 64), 231, "Opposite cardinals must retain a separate center hole");
  const maximumEdgeImage = "data:image/png;base64," + "A".repeat(4 * Math.ceil(512 * 1024 / 3));
  const maximum = { ...value, art: { ...value.art, terrainEdges: [{ ...source, image: maximumEdgeImage }] } };
  assert.equal(parseDstWorldMap(maximum, "a", "volcano", true).art.terrainEdges.length, 1,
    "A valid host edge image at the raw PNG limit needs its base64 and data-URL overhead");
  assert.throws(() => parseDstWorldMap({ ...maximum, art: { ...maximum.art,
    terrainEdges: [{ ...source, image: maximumEdgeImage + "AAAA" }] } }, "a", "volcano", true));
});

test("native per-terrain base materials are bounded and independent of noise registrations", () => {
  const value = valid();
  value.tileBaseTextures = { 3: "../mods/custom/levels/tiles/base.tex" };
  value.art.terrainBaseColors = { [value.tileBaseTextures[3]]: [255, 255, 255, 255] };
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true), value);
  for (const tileBaseTextures of [undefined, { 65536: "levels/tiles/base.tex" }, { 3: "x".repeat(513) }, { 3: "bad\npath" }]) {
    assert.throws(() => parseDstWorldMap({ ...value, tileBaseTextures }, "a", "volcano", true));
  }
  for (const terrainBaseColors of [undefined, { custom: [256, 0, 0, 255] }, { custom: [1, 2, 3] },
    { ["x".repeat(513)]: [1, 2, 3, 255] }, Object.fromEntries(Array.from({ length: 129 }, (_, id) => [String(id), [1, 2, 3, 255]]))]) {
    assert.throws(() => parseDstWorldMap({ ...value, art: { ...value.art, terrainBaseColors } }, "a", "volcano", true));
  }
});

test("terrain classification is required native evidence independent of tile names and IDs", () => {
  const value = { ...valid(), tiles: [50000, 50001, 50002, 50003],
    tileNames: { 50000: "FOREST", 50001: "OCEAN_ICE", 50002: "FLOWERS", 50003: "CUSTOM" },
    tileKinds: { 50000: "void", 50001: "land", 50002: "ocean", 50003: "unknown" },
    tileColors: { 50002: [1, 2, 3, 4] } };
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true).tileKinds, value.tileKinds);
  for (const tileKinds of [undefined, {}, { 1: "void" }, { 1: "void", 3: "grass" },
    { 1: "void", 3: "land", 65536: "land" }]) {
    assert.throws(() => parseDstWorldMap({ ...valid(), tileKinds }, "a", "volcano", true));
  }
  assert.throws(() => parseDstWorldMap({ ...valid(), tileColors: { 3: [1, 2, 3, 4] } }, "a", "volcano", true));
});
test("map responses cannot cross instances, shards, worlds or bounded terrain contracts", () => {
  for (const changed of [{ instanceId: "b" }, { shard: "caves" }, { width: 2049 }, { width: 2048, height: 2048 },
    { tiles: [1] }, { tiles: [1, 1, 1, 5] }, { tileNames: { "1": "bad\nname" } }, { tileScale: NaN },
    { capturedAtUnixMs: -1 }, { players: [{ ...valid().players[0], x: Infinity }] },
    { players: [valid().players[0], valid().players[0]] }]) {
    assert.throws(() => parseDstWorldMap({ ...valid(), ...changed }, "a", "volcano", true));
  }
  assert.throws(() => parseDstWorldMap(valid(), "a", "volcano", false));
  assert.equal(parseDstWorldMap({ ...valid(), tiles: null, tileNames: {}, tileKinds: {}, landmarks: null, roads: null, art: null }, "a", "volcano", false).tiles, null);
});

test("landmark, road and installed artwork boundaries reject malformed data", () => {
  for (const changed of [{ landmarks: [{ id: "x", prefab: "wormhole", icon: "wormhole.png", priority: 0, mapX: NaN, mapZ: 1 }] },
    { roads: [[[1, 2]]] }, { roads: [[[1, 2], [1, Infinity]]] },
    { art: { ...valid().art, textures: { grass: "https://example.test/tracker.png" } } },
    { art: { ...valid().art, icons: { "observed.png": "https://example.test/tracker.png" } } },
    { landmarksTruncated: null }]) assert.throws(() => parseDstWorldMap({ ...valid(), ...changed }, "a", "volcano", true));
});

test("player artwork uses a bounded observed icon name independent of the character prefab", () => {
  for (const icon of ["compass_avatar.png", null, "a".repeat(128)]) {
    const value = valid();
    value.players[0] = { ...value.players[0], character: "mod_cartographer", icon };
    assert.equal(parseDstWorldMap(value, "a", "volcano", true).players[0].icon, icon);
  }
  for (const icon of [undefined, "a".repeat(129), "bad\nicon.png", 1, {}]) {
    const value = valid();
    value.players[0] = { ...value.players[0], icon };
    assert.throws(() => parseDstWorldMap(value, "a", "volcano", true), /Invalid map player/);
  }
});

test("later player icons cannot borrow cached entity inventory art while genuine avatar sources remain usable", () => {
  const art = { ...valid().art, icons: { "Item.PNG": "inventory-pixels", "native.png": "avatar-pixels" },
    iconFallbacks: [{ icon: "Item.PNG", atlas: "../mods/enabled/item.xml", element: "item.tex", kind: "entity_inventory" }] };
  assert.equal(mapPlayerArtwork(art, "Item.PNG"), undefined);
  assert.equal(mapPlayerArtwork(art, "item.png"), undefined);
  assert.equal(mapPlayerArtwork(art, "native.png"), "avatar-pixels");
  assert.equal(mapPlayerArtwork(art, null), undefined);
  assert.equal(mapPlayerArtwork(null, "native.png"), undefined);
  assert.equal(mapPlayerArtwork(art, "toString"), undefined);
  for (const kind of ["registered_alias", "indexed_exact", "indexed_alias"]) {
    assert.equal(mapPlayerArtwork({ ...art, iconFallbacks: [{ ...art.iconFallbacks[0], kind }] }, "Item.PNG"), "inventory-pixels");
  }
});

test("native landmark priority, expanded collection and texture paths remain bounded", () => {
  const item = { id: "one", prefab: "mod_object", icon: "observed.png", priority: 0, mapX: 1, mapZ: 1 };
  const value = valid();
  value.landmarks = Array.from({ length: 32768 }, (_, id) => ({ ...item, id: String(id) }));
  assert.equal(parseDstWorldMap(value, "a", "volcano", true).landmarks.length, 32768);
  assert.throws(() => parseDstWorldMap({ ...value, landmarks: [...value.landmarks, { ...item, id: "overflow" }] }, "a", "volcano", true));
  for (const priority of [undefined, null, NaN, Infinity, 1e7 + 1, -1e7 - 1]) {
    assert.throws(() => parseDstWorldMap({ ...valid(), landmarks: [{ ...item, priority }] }, "a", "volcano", true));
  }
  const texture = "m".repeat(512);
  const textured = valid(); textured.tileTextures = { 3: texture }; textured.art.textures = { [texture]: "data:image/png;base64,AA==" };
  assert.equal(parseDstWorldMap(textured, "a", "volcano", true).tileTextures[3], texture);
  assert.throws(() => parseDstWorldMap({ ...textured, tileTextures: { 3: `${texture}x` } }, "a", "volcano", true));
});

test("landmark selection uses observed priority and stable id ties, independent of prefab or input order", () => {
  const item = { id: "z", prefab: "multiplayer_portal", icon: "icon.png", priority: 0, mapX: 1, mapZ: 1 };
  const items = [item, { ...item, id: "b", prefab: "mod_custom", priority: 5 }, { ...item, id: "a", prefab: "mod_other", priority: 5 }];
  const view = { centerX: 0, centerZ: 0, width: 100, height: 100, scale: 1 };
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), view).map(item => item.id), ["a"]);
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items.reverse()), view).map(item => item.id), ["a"]);
  const edges = [{ ...item, id: "partial", mapX: -53.9 }, { ...item, id: "outside", mapX: -54.1, priority: 100 }];
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(edges), view).map(item => item.id), ["partial"]);
});

test("dense native landmarks stay bounded by screen cells while zoom reveals local detail", () => {
  const items = Array.from({ length: 10000 }, (_, id) => ({ id: String(id), prefab: "mod_object", icon: "icon.png", priority: 0,
    mapX: id % 100 * 10 + 5, mapZ: Math.floor(id / 100) * 10 + 5 }));
  const index = indexDstLandmarks(items);
  const view = { centerX: 500, centerZ: 500, width: 320, height: 320, scale: .32 };
  const fit = visibleDstLandmarks(index, view), zoom = visibleDstLandmarks(index, { ...view, scale: 1.28 });
  for (const [items, scale] of [[fit, view.scale], [zoom, 1.28]]) {
    const size = dstLandmarkIconSize(scale);
    assert.ok(items.length * size * size < view.width * view.height * .25, "Icons must leave most of a dense map visible");
  }
  const local = values => values.filter(item => item.mapX >= 400 && item.mapX <= 600 && item.mapZ >= 400 && item.mapZ <= 600).length;
  assert.ok(local(zoom) > local(fit), "zoom should reveal more objects inside the same native world area");
  assert.ok(zoom.every(item => Math.abs(item.mapX - 500) <= 130 && Math.abs(item.mapZ - 500) <= 130));
});

test("native priority resolves overlapping different icons without hiding the terrain under a stack", () => {
  const item = { id: "a", prefab: "mod_tree", icon: "tree.png", priority: 0, mapX: 209, mapZ: 400 };
  const items = [item, { ...item, id: "b", prefab: "mod_building", icon: "building.png" },
    { ...item, id: "c", priority: 5 }, { ...item, id: "d", priority: 5 }];
  const view = { centerX: 209, centerZ: 400, width: 1232, height: 560, scale: 4 };
  for (const ordered of [items, [...items].reverse()]) {
    assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(ordered), view).map(item => item.id), ["c"]);
  }
});

test("varied native icons retain representatives within the visible SVG budget", () => {
  const items = Array.from({ length: 960 }, (_, id) => ({ id: String(id), prefab: "mod_object",
    icon: `icon-${id % 48}.png`, priority: id % 3, mapX: id % 48 * 35, mapZ: Math.floor(id / 48) * 35 }));
  const view = { centerX: 840, centerZ: 560, width: 2000, height: 1400, scale: 1 };
  const first = visibleDstLandmarks(indexDstLandmarks(items), view);
  const reversed = visibleDstLandmarks(indexDstLandmarks([...items].reverse()), view);
  assert.ok(first.length > 0 && first.length <= 1024);
  assert.equal(new Set(first.map(item => item.icon)).size, 48);
  assert.deepEqual(first.map(item => item.id), reversed.map(item => item.id));
});

test("more distinct native icons than the SVG budget still have a stable hard limit", () => {
  const items = Array.from({ length: 1025 }, (_, id) => ({ id: String(id), prefab: "mod_object",
    icon: `icon-${id}.png`, priority: 0, mapX: id % 33 * 40, mapZ: Math.floor(id / 33) * 40 }));
  const view = { centerX: 640, centerZ: 640, width: 1400, height: 1400, scale: 1 };
  const first = visibleDstLandmarks(indexDstLandmarks(items), view);
  assert.equal(first.length, 1024);
  assert.deepEqual(first.map(item => item.id), visibleDstLandmarks(indexDstLandmarks(items.reverse()), view).map(item => item.id));
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), { ...view, scale: Infinity }), []);
});

test("decluttering terminates and respects priority across signed coordinate quadrants", () => {
  const items = Array.from({ length: 1200 }, (_, id) => ({ id: String(id), prefab: "mod_object",
    icon: `icon-${id % 300}.png`, priority: 0, mapX: id < 600 ? -1 : 1, mapZ: Math.floor(id / 300) % 2 ? -1 : 1 }));
  const view = { centerX: 0, centerZ: 0, width: 320, height: 320, scale: 1 };
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), view).map(item => item.id), ["0"]);
});

test("landmark sizes shrink with terrain, recover separate icons on zoom and remain bounded", () => {
  assert.equal(dstLandmarkIconSize(.5), 4);
  assert.equal(dstLandmarkIconSize(1), 8);
  assert.equal(dstLandmarkIconSize(2), 16);
  assert.equal(dstLandmarkIconSize(10), 24);
  for (const scale of [0, NaN, Infinity, -1]) assert.equal(dstLandmarkIconSize(scale), 0);
  const items = [{ id: "one", prefab: "mod_a", icon: "a.png", priority: 0, mapX: 20, mapZ: 20 },
    { id: "two", prefab: "mod_b", icon: "b.png", priority: 1, mapX: 26, mapZ: 20 }];
  const view = { centerX: 23, centerZ: 20, width: 300, height: 300, scale: 1 };
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), view).map(item => item.id), ["two"]);
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), { ...view, scale: 5 }).map(item => item.id), ["one", "two"]);
});

test("equal-priority repeated resources cannot crowd out a scarce observed Mod icon", () => {
  const common = { id: "a", prefab: "mod_resource", icon: "resource.png", priority: 0, mapX: 20, mapZ: 20 };
  const unique = { ...common, id: "z", prefab: "mod_building", icon: "unique.png" };
  const items = [common, unique, { ...common, id: "b", mapX: 60 }, { ...common, id: "c", mapX: 90 }];
  const view = { centerX: 20, centerZ: 20, width: 40, height: 40, scale: 1 };
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items), view).map(item => item.id), ["z"]);
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks([...items].reverse()), view).map(item => item.id), ["z"]);
  assert.deepEqual(visibleDstLandmarks(indexDstLandmarks(items.map(item => item.id === "a" ? { ...item, priority: 1 } : item)), view).map(item => item.id), ["a"]);
});

test("zoom bounds, reversible steps and pointer anchoring share one world-coordinate contract", () => {
  assert.equal(clampDstMapZoom(-10), 1);
  assert.equal(clampDstMapZoom(100), 10);
  assert.equal(clampDstMapZoom(NaN), 1);
  assert.equal(stepDstMapZoom(stepDstMapZoom(2, 1), -1), 2);
  assert.deepEqual(anchoredDstMapCenter({ x: 50, y: 40 }, { x: 40, y: -20 }, 2, 4), { x: 60, y: 35 });
  assert.deepEqual(anchoredDstMapCenter({ x: 50, y: 40 }, { x: 40, y: -20 }, 0, 4), { x: 50, y: 40 });
});

test("native artwork keeps separate texture, icon and issue budgets", () => {
  const images = count => Object.fromEntries(Array.from({ length: count }, (_, index) => [`asset-${index}`, "data:image/png;base64,AA=="]));
  const value = valid();
  value.art = { ...value.art, textures: images(128), icons: images(512), issues: Array(1024).fill("bounded notice") };
  assert.equal(Object.keys(parseDstWorldMap(value, "a", "volcano", true).art.icons).length, 512);
  for (const changed of [{ textures: images(129) }, { icons: images(513) }, { issues: Array(1025).fill("notice") }, { issues: ["x".repeat(2049)] }]) {
    assert.throws(() => parseDstWorldMap({ ...value, art: { ...value.art, ...changed } }, "a", "volcano", true));
  }
});

test("completed icons carry bounded, unique source metadata for actual returned artwork", () => {
  const value = valid();
  value.art.icons["building.png"] = "data:image/png;base64,AA==";
  const source = { icon: "building.png", element: "building.tex", atlas: "../mods/example/images/atlas.xml", kind: "registered_alias" };
  for (const kind of ["registered_alias", "indexed_exact", "indexed_alias"]) {
    value.art.iconFallbacks = [{ ...source, kind }];
    assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true).art.iconFallbacks, value.art.iconFallbacks);
  }
  for (const changed of [{ icon: "unknown.png" }, { icon: "" }, { element: "" }, { atlas: "" }, { atlas: "x".repeat(513) },
    { kind: "native" }, { kind: ["registered_alias"] }, { element: "bad\nname.tex" }]) {
    value.art.iconFallbacks = [{ ...source, ...changed }];
    assert.throws(() => parseDstWorldMap(value, "a", "volcano", true), /icon completion/);
  }
  for (const sources of [undefined, null, [source, source], Array(513).fill(source)]) {
    value.art.iconFallbacks = sources;
    assert.throws(() => parseDstWorldMap(value, "a", "volcano", true), /icon completion/);
  }
});

test("entity inventory icon completion requires a complete shared declaration and separate avatar identity", () => {
  const declared = () => {
    const value = valid();
    value.landmarks = [{ id: "item-a", prefab: "actual_item", icon: "missing.png", priority: 0, mapX: 1, mapZ: 1,
      inventoryIcon: { atlas: "../mods/example/images/item.xml", element: "item.tex" } }];
    value.art.icons["missing.png"] = "data:image/png;base64,AA==";
    value.art.iconFallbacks = [{ icon: "missing.png", atlas: "../mods/example/images/item.xml", element: "ITEM.TEX", kind: "entity_inventory" }];
    return value;
  };
  const value = declared();
  value.landmarks.push({ ...value.landmarks[0], id: "item-b", icon: "MISSING.PNG" });
  assert.deepEqual(parseDstWorldMap(value, "a", "volcano", true), value);
  for (const mutate of [
    v => { v.landmarks.push({ ...v.landmarks[0], id: "item-b", inventoryIcon: null }); },
    v => { v.landmarks.push({ ...v.landmarks[0], id: "item-b", inventoryIcon: undefined }); },
    v => { v.landmarks.push({ ...v.landmarks[0], id: "item-b", icon: "MISSING.PNG", inventoryIcon: { atlas: "other.xml", element: "item.tex" } }); },
    v => { v.landmarks.push({ ...v.landmarks[0], id: "item-b", inventoryIcon: { atlas: v.landmarks[0].inventoryIcon.atlas, element: "other.tex" } }); },
    v => { v.landmarks = []; },
    v => { v.players[0].icon = "Missing.PNG"; },
    v => { v.landmarksTruncated = true; },
    v => { v.art.iconFallbacks[0].atlas = "unrelated.xml"; },
    v => { v.art.iconFallbacks[0].element = "unrelated.tex"; },
  ]) {
    const invalid = declared(); mutate(invalid);
    assert.throws(() => parseDstWorldMap(invalid, "a", "volcano", true), /Unproven entity inventory icon completion/);
    // Existing successful source selection has authority of its own.
    invalid.art.iconFallbacks[0].kind = "indexed_exact";
    assert.doesNotThrow(() => parseDstWorldMap(invalid, "a", "volcano", true));
  }
});

test("optional entity inventory declarations reject malformed fields without guessing absent values", () => {
  for (const inventoryIcon of [undefined, null, { atlas: "images/item.xml", element: "item.tex" },
    { atlas: "图".repeat(170) + "ab", element: "界".repeat(42) + "ab" }]) {
    const value = valid();
    value.landmarks = [{ id: "a", prefab: "item", icon: "item.png", priority: 0, mapX: 1, mapZ: 1, inventoryIcon }];
    assert.doesNotThrow(() => parseDstWorldMap(value, "a", "volcano", true));
  }
  for (const inventoryIcon of [false, [], {}, { atlas: "", element: "a" }, { atlas: "a", element: "" },
    { atlas: "x".repeat(513), element: "a" }, { atlas: "a", element: "x".repeat(129) }, { atlas: "a\nb", element: "a" },
    { atlas: "图".repeat(171), element: "a" }, { atlas: "a", element: "界".repeat(43) },
    { atlas: "a", element: "bad\u0000image" }, { atlas: "a", element: "bad\u0085image" },
    { atlas: "a", element: "a", unrelated: true }, { atlas: 3, element: "a" }]) {
    const value = valid();
    value.landmarks = [{ id: "a", prefab: "item", icon: "item.png", priority: 0, mapX: 1, mapZ: 1, inventoryIcon }];
    assert.throws(() => parseDstWorldMap(value, "a", "volcano", true), /Invalid (map inventory artwork declaration|world map response)/);
  }
});
