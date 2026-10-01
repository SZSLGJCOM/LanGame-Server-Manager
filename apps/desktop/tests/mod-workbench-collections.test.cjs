const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".tsx"] = require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const {
  readManagedWorkshopCollections,
  collectInstalledWorkshopCollections,
  mergeManagedWorkshopCollections,
  removeManagedWorkshopCollection
} = require("../src/views/servers/mod-workbench-collections.ts");
const { inspectWorkshopManifest } = require("../src/views/servers/workshop-manifest.ts");

const APP_ID = 322330;
const A = "1000001", B = "1000002", X = "2000001", Y = "2000002", Z = "2000003";
const record = (id, member_ids = [], title = id) => ({ id, title, member_ids });
const item = (id, overrides = {}) => ({
  id, title: `Mod ${id}`, status: "resolved", item_kind: "item", consumer_app_id: APP_ID,
  detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`,
  child_count: 0, children: [], ...overrides
});
const collection = (id, children, overrides = {}) => item(id, {
  item_kind: "collection", children, child_count: children.length, ...overrides
});

test("server collection provenance excludes explicit client-only children and refreshes full child metadata", () => {
  const root = collection(A, [item(X), item(Y, { tags: ["CLIENT_ONLY_MOD"] }), item(Z)]);
  const lookup = { [A]: root, [Z]: item(Z, { tags: ["client_only_mod"] }) };
  assert.deepEqual(collectInstalledWorkshopCollections([A], lookup, APP_ID), [record(A, [X], root.title)]);
  // A full server response supersedes an old client-only child summary.
  lookup[Y] = item(Y);
  assert.deepEqual(collectInstalledWorkshopCollections([A], lookup, APP_ID)[0].member_ids, [X, Y]);
});

test("client tags cannot disguise missing, unverified, foreign or unsupported members", () => {
  const cases = [
    ["missing", { status: "not_found" }], ["unresolved", { status: "unverified" }],
    ["unresolved", { consumer_app_id: null }], ["wrong-game", { consumer_app_id: 108600 }],
    ["unsupported", { status: "unsupported", item_kind: "guide" }],
    ["unsupported", { item_kind: "guide" }]
  ];
  for (const [reason, overrides] of cases) {
    const bad = item(Y, { tags: ["client_only_mod"], ...overrides });
    assert.throws(() => collectInstalledWorkshopCollections([A], { [A]: collection(A, [item(X), bad]) }, APP_ID), (error) => {
      assert.deepEqual(JSON.parse(error.message), {
        code: "workshop-collection-install", reason, item_id: Y,
        message: `Workshop collection member ${Y}: ${reason}.`
      });
      return error.reason === reason && error.item_id === Y;
    });
  }
});

test("direct client-only requests and collections without any server content cannot claim an installation", () => {
  const client = item(X, { tags: ["client_only_mod"] });
  assert.throws(() => collectInstalledWorkshopCollections([X], { [X]: client }, APP_ID), { reason: "client-only", item_id: X });
  assert.throws(() => collectInstalledWorkshopCollections([A], { [A]: collection(A, [client]) }, APP_ID),
    { reason: "empty-collection", item_id: A });
  assert.deepEqual(collectInstalledWorkshopCollections([A], {
    [A]: collection(A, [item(X, { consumer_app_id: 108600, tags: ["client_only_mod"] })], { consumer_app_id: 108600 })
  }, 108600)[0].member_ids, [X], "DST client classification does not apply to another game");
});

test("offline records retain snapshots and only explicit DST collection IDs provide fallback ownership", () => {
  const settings = {
    steam_workshop_collections: [record(A, [X, Y], "Saved title")],
    shared_workshop_collection_ids: `${A}\n${B}\n${B}`,
    shared_workshop_mod_ids: X,
    master_enabled_workshop_mod_ids: `${X}\n${Y}`
  };
  assert.deepEqual(readManagedWorkshopCollections(settings, "dontstarve"), [
    record(A, [X, Y], "Saved title"), record(B)
  ]);
  assert.deepEqual(readManagedWorkshopCollections(settings, "squad"), [record(A, [X, Y], "Saved title")]);
  assert.deepEqual(readManagedWorkshopCollections({ master_enabled_workshop_mod_ids: `${X}\n${Y}` }, "dontstarve"), []);
  assert.deepEqual(readManagedWorkshopCollections({ steam_workshop_collections: null }, "unturned"), []);
});

test("record reading validates whole snapshots, canonical IDs and bounds without retaining aliases", () => {
  const settings = { steam_workshop_collections: [
    record(A, [X, X, Y], "First"), record(A, [Z], "Duplicate"),
    record("01000002"), record("18446744073709551616"), record(B, ["invalid"]),
    { ...record(B), injected: true }, { id: B, title: 42, member_ids: [] },
    record(B, [], "😀".repeat(513)), record("1000003", [], "😀".repeat(512))
  ] };
  const records = readManagedWorkshopCollections(settings, "unturned");
  assert.deepEqual(records, [record(A, [X, Y], "First"), record("1000003", [], "😀".repeat(512))]);
  records[0].member_ids.push(Z);
  assert.deepEqual(settings.steam_workshop_collections[0].member_ids, [X, X, Y], "reads never expose mutable stored arrays");
});

test("normal browse installation captures only explicitly chosen collections and embedded leaf metadata", () => {
  const root = collection(A, [item(X), item(Y), item(X)], { title: "A collection" });
  const captured = collectInstalledWorkshopCollections([X, A, A], { [A]: root, [X]: item(X) }, APP_ID);
  assert.deepEqual(captured, [record(A, [X, Y], "A collection")]);
  assert.deepEqual(collectInstalledWorkshopCollections([X], { [X]: item(X) }, APP_ID), []);
  assert.deepEqual(collectInstalledWorkshopCollections([], {}, APP_ID), []);
});

test("real manifest review preserves root provenance and ordered nested members without owning implicit collections", async () => {
  const catalog = {
    [A]: collection(A, [item(X), item(B, { item_kind: "collection" }), item(Z)], { title: "Root" }),
    [B]: collection(B, [item(Y), item(X)]), [X]: item(X), [Y]: item(Y), [Z]: item(Z)
  };
  const review = await inspectWorkshopManifest([A, Z], APP_ID, {
    lookup: async (ids) => ids.map((id) => catalog[id]),
    inspect: async (ids) => ({ consumer_app_id: APP_ID, searched_roots: [], items: ids.map((item_id) => ({ item_id, installed: true })) }),
    isCurrent: () => true
  });
  assert.deepEqual(review.issues, []);
  assert.deepEqual(collectInstalledWorkshopCollections(review.ids, review.items, APP_ID), [record(A, [X, Y, Z], "Root")]);
  assert.deepEqual(collectInstalledWorkshopCollections([A, B], review.items, APP_ID).map((entry) => entry.id), [A, B]);
});

test("failed root verification cannot create a successful provenance record", () => {
  for (const root of [
    undefined,
    collection(A, [item(X)], { status: "unverified" }),
    collection(A, [item(X)], { consumer_app_id: 108600 }),
    collection(A, [item(X)], { consumer_app_id: null }),
    collection(A, [item(X)], { item_kind: "guide" }),
    collection(A, [item(X)], { id: B }),
    collection(A, [item(X)], { tags: ["client_only_mod"] })
  ]) {
    assert.throws(() => collectInstalledWorkshopCollections([A], root ? { [A]: root } : {}, APP_ID));
  }
  for (const invalidApp of [null, 0, -1, NaN, 1.5]) {
    assert.throws(() => collectInstalledWorkshopCollections([A], { [A]: collection(A, [item(X)]) }, invalidApp));
  }
  for (const invalidId of ["0", "10000", "0100001", "18446744073709551616"]) {
    assert.throws(() => collectInstalledWorkshopCollections([invalidId], { [invalidId]: collection(invalidId, [item(X)]) }, APP_ID));
  }
});

test("incomplete, empty, unresolved, cross-game and cyclic graphs are rejected even when some members are valid", () => {
  const graphs = [
    { [A]: collection(A, [item(X)], { child_count: 2 }) },
    { [A]: collection(A, []) },
    { [A]: collection(A, [item(X), item(Y, { status: "not_found" })]) },
    { [A]: collection(A, [item(X), item(Y, { consumer_app_id: 108600 })]) },
    { [A]: collection(A, [item(X), item(B, { item_kind: "collection" })]) },
    { [A]: collection(A, [item(X), item(B, { item_kind: "collection" })]), [B]: collection(B, [item(A, { item_kind: "collection" })]) },
    { [A]: collection(A, [item(X)]), [X]: item(X, { status: "unsupported" }) }
  ];
  for (const graph of graphs) assert.throws(() => collectInstalledWorkshopCollections([A], graph, APP_ID));
});

test("collection traversal accepts shared subgraphs but bounds nesting and member snapshots", () => {
  const shared = {
    [A]: collection(A, [item(B, { item_kind: "collection" }), item(B, { item_kind: "collection" }), item(X)]),
    [B]: collection(B, [item(Y)])
  };
  assert.deepEqual(collectInstalledWorkshopCollections([A], shared, APP_ID)[0].member_ids, [Y, X]);
  const deep = {};
  for (let index = 0; index < 33; index++) {
    const id = String(1000000 + index), childId = String(1000001 + index);
    deep[id] = collection(id, [item(childId, { item_kind: "collection" })]);
  }
  deep["1000033"] = collection("1000033", [item(X)]);
  assert.throws(() => collectInstalledWorkshopCollections(["1000000"], deep, APP_ID));
  const members = Array.from({ length: 8192 }, (_, index) => item(String(2000000 + index)));
  assert.equal(collectInstalledWorkshopCollections([A], { [A]: collection(A, members) }, APP_ID)[0].member_ids.length, 8192);
  assert.throws(() => collectInstalledWorkshopCollections([A], { [A]: collection(A, [...members, item("3000000")]) }, APP_ID));
});

test("snapshot titles use Unicode characters and a usable offline fallback", () => {
  assert.equal(collectInstalledWorkshopCollections([A], { [A]: collection(A, [item(X)], { title: null }) }, APP_ID)[0].title, A);
  assert.equal(collectInstalledWorkshopCollections([A], { [A]: collection(A, [item(X)], { title: " " }) }, APP_ID)[0].title, A);
  assert.equal(collectInstalledWorkshopCollections([A], { [A]: collection(A, [item(X)], { title: "😀".repeat(513) }) }, APP_ID)[0].title, "😀".repeat(512));
});

test("merge deduplicates IDs, replaces only an explicitly refreshed snapshot, and is idempotent", () => {
  const settings = { name: "Unchanged", steam_workshop_collections: [record(A, [X], "Old"), record(B, [Y], "B")] };
  const before = structuredClone(settings);
  const merged = mergeManagedWorkshopCollections(settings, [record(A, [X, X, Z], "Updated")]);
  assert.deepEqual(merged, { ...settings, steam_workshop_collections: [record(A, [X, Z], "Updated"), record(B, [Y], "B")] });
  assert.deepEqual(settings, before);
  assert.ok(Object.is(mergeManagedWorkshopCollections(merged, [record(A, [X, Z], "Updated")]), merged));
  assert.ok(Object.is(mergeManagedWorkshopCollections(settings, []), settings));
  assert.throws(() => mergeManagedWorkshopCollections(settings, [record(A, ["bad"])]));
  assert.throws(() => mergeManagedWorkshopCollections({ steam_workshop_collections: "broken" }, [record(A)]));
});

test("record count and total member bounds apply across new and existing provenance", () => {
  const records = Array.from({ length: 128 }, (_, index) => record(String(1000000 + index)));
  assert.equal(mergeManagedWorkshopCollections({}, records).steam_workshop_collections.length, 128);
  assert.throws(() => mergeManagedWorkshopCollections({ steam_workshop_collections: records }, [record("9999999")]));
  const members = Array.from({ length: 8192 }, (_, index) => String(2000000 + index));
  const snapshots = Array.from({ length: 8 }, (_, index) => record(String(1000000 + index), members));
  const settings = mergeManagedWorkshopCollections({}, snapshots);
  assert.equal(settings.steam_workshop_collections.reduce((sum, entry) => sum + entry.member_ids.length, 0), 65536);
  assert.throws(() => mergeManagedWorkshopCollections(settings, [record("9999999", [X])]));
  assert.throws(() => mergeManagedWorkshopCollections({}, [record(A, [...members, Z])]));
});

test("removing an overlapping DST collection only unlinks that source and its native download request", () => {
  const settings = {
    steam_workshop_collections: [record(A, [X, Y], "A"), record(B, [Y, Z], "B")],
    shared_workshop_collection_ids: `${A}\n${B}`,
    shared_workshop_mod_ids: `${X}\n${Y}\n${Z}`,
    master_enabled_workshop_mod_ids: `${X}\n${Y}\n${Z}`,
    caves_enabled_workshop_mod_ids: Y,
    master_mod_configuration_options: { [Y]: { difficulty: 3 } },
    caves_mod_configuration_options: { [Y]: { difficulty: 2 } },
    master_modoverrides_lua: "preserve raw settings", unrelated: { value: 1 }
  };
  const before = structuredClone(settings);
  const removed = removeManagedWorkshopCollection(settings, "dontstarve", A);
  assert.deepEqual(removed, { ...settings, steam_workshop_collections: [record(B, [Y, Z], "B")], shared_workshop_collection_ids: B });
  assert.deepEqual(settings, before);
  assert.deepEqual(readManagedWorkshopCollections(removed, "dontstarve"), [record(B, [Y, Z], "B")]);
  assert.deepEqual(removeManagedWorkshopCollection(removed, "dontstarve", A), removed);
});

test("offline native fallback can be unlinked without manufacturing a snapshot or changing member IDs", () => {
  const settings = { shared_workshop_collection_ids: A, master_enabled_workshop_mod_ids: X };
  assert.deepEqual(removeManagedWorkshopCollection(settings, "dontstarve", A), {
    shared_workshop_collection_ids: "", master_enabled_workshop_mod_ids: X
  });
  assert.deepEqual(removeManagedWorkshopCollection(settings, "unturned", A), settings);
  assert.throws(() => removeManagedWorkshopCollection(settings, "dontstarve", "0100001"));
});
