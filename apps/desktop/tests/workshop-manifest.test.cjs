const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".tsx"] = require.extensions[".ts"] = function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

const { parseWorkshopManifest, inspectWorkshopManifest } = require("../src/views/servers/workshop-manifest.ts");

function item(id, overrides = {}) {
  return {
    id, status: "resolved", item_kind: "item", consumer_app_id: 322330,
    detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`,
    child_count: 0, children: [], ...overrides
  };
}

function inventory(installedIds = [], appId = 322330) {
  return {
    consumer_app_id: appId, searched_roots: ["fixture/ugc_mods/main/Master/content/322330"],
    items: installedIds.map((item_id) => ({ item_id, installed: true, path: `fixture/${item_id}` }))
  };
}

function dependencies(catalog, installedIds = []) {
  return {
    lookup: async (ids) => ids.flatMap((id) => catalog[id] ? [catalog[id]] : []),
    inspect: async () => inventory(installedIds),
    isCurrent: () => true
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

test("manifest parser accepts IDs, prefixes and exact Steam URLs with stable deduplication", () => {
  const parsed = parseWorkshopManifest([
    "3739491677,workshop-3734727477;https://steamcommunity.com/sharedfiles/filedetails/?id=378160973",
    "3739491677\thttps://www.steamcommunity.com/workshop/filedetails?id=378160973",
    "WORKSHOP-3734727477，steamcommunity.com/sharedfiles/filedetails/?id=1000001；1000002"
  ].join("\n"));
  assert.deepEqual(parsed, {
    ids: ["3739491677", "3734727477", "378160973", "1000001", "1000002"],
    invalidTokens: [], duplicateCount: 3, tooLarge: false
  });
});

test("manifest parser never extracts IDs from unrelated or ambiguous URLs", () => {
  const invalid = [
    "https://example.com/mod/3739491677",
    "https://example.com/?url=https://steamcommunity.com/sharedfiles/filedetails/?id=3739491677",
    "https://steamcommunity.com.evil.example/sharedfiles/filedetails/?id=3739491677",
    "https://steamcommunity.com/app/322330",
    "https://steamcommunity.com/sharedfiles/filedetails/?id=3739491677&id=378160973",
    "https://user@steamcommunity.com/sharedfiles/filedetails/?id=3739491677",
    "https://steamcommunity.com:8443/sharedfiles/filedetails/?id=3739491677",
    "ftp://steamcommunity.com/sharedfiles/filedetails/?id=3739491677",
    "mod-3739491677", "18446744073709551616", "0", "-3739491677"
  ];
  const parsed = parseWorkshopManifest(invalid.join("\n"));
  assert.deepEqual(parsed.ids, []);
  assert.deepEqual(parsed.invalidTokens, invalid);
  assert.equal(parseWorkshopManifest("18446744073709551615").ids[0], "18446744073709551615");
});

test("manifest parser enforces UTF-8 byte and distinct-ID limits", () => {
  assert.equal(parseWorkshopManifest(" ".repeat(1024 * 1024)).tooLarge, false);
  assert.equal(parseWorkshopManifest(" ".repeat(1024 * 1024 + 1)).tooLarge, true);
  assert.equal(parseWorkshopManifest("模".repeat(Math.floor(1024 * 1024 / 3) + 1)).tooLarge, true);
  const ids = Array.from({ length: 8193 }, (_, index) => String(1000000 + index));
  assert.equal(parseWorkshopManifest(ids.slice(0, 8192).join("\n")).tooLarge, false);
  const oversized = parseWorkshopManifest(ids.join("\n"));
  assert.equal(oversized.tooLarge, true);
  assert.equal(oversized.ids.length, 8192);
  const repeated = parseWorkshopManifest(Array(9000).fill("3739491677").join("\n"));
  assert.equal(repeated.tooLarge, false);
  assert.equal(repeated.duplicateCount, 8999);
});

test("manifest review confirms leaf metadata and treats omitted inventory entries as missing", async () => {
  const catalog = { "1000001": item("1000001"), "1000002": item("1000002") };
  const review = await inspectWorkshopManifest(["1000001", "1000002", "1000001"], 322330, dependencies(catalog, ["1000001", "9999999"]));
  assert.deepEqual(review.ids, ["1000001", "1000002"]);
  assert.deepEqual(review.contentIds, review.ids);
  assert.deepEqual(review.installedIds, ["1000001"]);
  assert.deepEqual(review.missingIds, ["1000002"]);
  assert.deepEqual(review.issues, []);
  assert.deepEqual(review.searchedRoots, inventory().searched_roots);
});

test("manifest review re-looks up collection children and reports each unusable child", async () => {
  const childIds = ["1000002", "1000003", "1000004", "1000005", "1000006", "1000007", "1000008"];
  const catalog = {
    "1000001": item("1000001", {
      item_kind: "collection", child_count: childIds.length,
      children: childIds.map((id) => ({ id, item_kind: "item", status: "resolved", consumer_app_id: 322330 }))
    }),
    "1000002": item("1000002"),
    "1000003": item("1000003", { tags: ["client_only_mod"] }),
    "1000004": item("1000004", { item_kind: "guide", status: "unsupported" }),
    "1000005": item("1000005", { consumer_app_id: 108600 }),
    "1000007": item("1000007", { status: "pending" }),
    "1000008": item("1000008", { consumer_app_id: null }),
    "1000009": item("1000009", { item_kind: "collection" })
  };
  const calls = [];
  const deps = dependencies(catalog);
  const review = await inspectWorkshopManifest(["1000001", "1000009"], 322330, {
    ...deps, lookup: async (ids) => { calls.push(ids); return deps.lookup(ids); }
  });
  assert.deepEqual(calls, [["1000001", "1000009"], childIds]);
  assert.deepEqual(review.contentIds, ["1000002"]);
  assert.deepEqual(review.skippedClientOnlyIds, ["1000003"]);
  assert.deepEqual(review.issues, [
    { id: "1000009", reason: "empty-collection" },
    { id: "1000004", reason: "unsupported" },
    { id: "1000005", reason: "wrong-game" },
    { id: "1000006", reason: "unresolved" },
    { id: "1000007", reason: "unresolved" },
    { id: "1000008", reason: "unresolved" }
  ]);
  assert.equal(review.items["1000003"].tags[0], "client_only_mod");
});

test("manifest server collections skip nested client-only branches while explicit client roots remain blocked", async () => {
  const collection = (id, ids) => item(id, { item_kind: "collection", child_count: ids.length,
    children: ids.map(id => ({ id, status: "resolved", item_kind: "item" })) });
  const catalog = {
    "1000001": collection("1000001", ["1000002", "1000003"]),
    "1000002": item("1000002"),
    "1000003": collection("1000003", ["1365141672"]),
    "1365141672": item("1365141672", { tags: ["CLIENT_ONLY_MOD"] })
  };
  let inspected;
  const deps = { ...dependencies(catalog), inspect: async ids => { inspected = ids; return inventory(); } };
  const review = await inspectWorkshopManifest(["1000001"], 322330, deps);
  assert.deepEqual(review.issues, []);
  assert.deepEqual(review.contentIds, ["1000002"]);
  assert.deepEqual(inspected, ["1000002"]);
  assert.deepEqual(review.skippedClientOnlyIds, ["1365141672"]);
  const { collectInstalledWorkshopCollections } = require("../src/views/servers/mod-workbench-collections.ts");
  assert.deepEqual(collectInstalledWorkshopCollections(review.ids, review.items, 322330)[0].member_ids, ["1000002"]);
  const direct = await inspectWorkshopManifest(["1000001", "1365141672"], 322330, deps);
  assert.deepEqual(direct.issues, [{ id: "1365141672", reason: "client-only" }]);
  assert.deepEqual(direct.skippedClientOnlyIds, []);
  const empty = await inspectWorkshopManifest(["1000003"], 322330, deps);
  assert.deepEqual(empty.contentIds, []);
  assert.deepEqual(empty.issues, [{ id: "1000003", reason: "empty-collection" }]);
});

test("manifest client-only skipping never hides unusable metadata", async () => {
  for (const [reason, overrides] of [
    ["unresolved", { status: "not_found" }], ["unresolved", { consumer_app_id: null }],
    ["wrong-game", { consumer_app_id: 108600 }], ["unsupported", { item_kind: "guide" }]
  ]) {
    const catalog = {
      "1000001": item("1000001", { item_kind: "collection", child_count: 2, children: [{ id: "1000002" }, { id: "1365141672" }] }),
      "1000002": item("1000002"),
      "1365141672": item("1365141672", { tags: ["client_only_mod"], ...overrides })
    };
    const review = await inspectWorkshopManifest(["1000001"], 322330, dependencies(catalog));
    assert.deepEqual(review.issues, [{ id: "1365141672", reason }]);
    assert.deepEqual(review.skippedClientOnlyIds, []);
  }
});

test("manifest review expands nested collections once and rejects empty cycles", async () => {
  const child = (id) => ({ id, item_kind: "collection", status: "resolved" });
  const catalog = {
    "1000001": item("1000001", { item_kind: "collection", children: [child("1000002"), child("1000003")], child_count: 2 }),
    "1000002": item("1000002", { item_kind: "collection", children: [child("1000003")], child_count: 1 }),
    "1000003": item("1000003"),
    "1000004": item("1000004", { item_kind: "collection", children: [child("1000004")], child_count: 1 })
  };
  const review = await inspectWorkshopManifest(["1000001", "1000004"], 322330, dependencies(catalog));
  assert.deepEqual(review.contentIds, ["1000003"]);
  assert.deepEqual(review.issues, [{ id: "1000004", reason: "empty-collection" }]);
});

test("manifest review bounds lookup batches and concurrency while preserving order", async () => {
  const ids = Array.from({ length: 321 }, (_, index) => String(1000000 + index));
  const calls = [];
  let active = 0;
  let maximum = 0;
  const reviewPromise = inspectWorkshopManifest(ids, 322330, {
    lookup: async (batch) => {
      const gate = deferred();
      calls.push({ batch, gate });
      maximum = Math.max(maximum, ++active);
      await gate.promise;
      active -= 1;
      return batch.map((id) => item(id));
    },
    inspect: async () => inventory(), isCurrent: () => true
  });
  assert.equal(calls.length, 4);
  calls[3].gate.resolve();
  await new Promise(setImmediate);
  assert.equal(calls.length, 5);
  calls[4].gate.resolve();
  await new Promise(setImmediate);
  assert.equal(calls.length, 6);
  for (const call of calls) call.gate.resolve();
  const review = await reviewPromise;
  assert.equal(maximum, 4);
  assert.ok(calls.every((call) => call.batch.length <= 64));
  assert.deepEqual(review.contentIds, ids);
});

test("manifest review preserves root and nested collection load order with shared children", async () => {
  const collection = (id, ids) => item(id, {
    item_kind: "collection", child_count: ids.length,
    children: ids.map((childId) => ({ id: childId, item_kind: "item", status: "resolved" }))
  });
  const catalog = {
    "1000001": collection("1000001", ["1000003", "1000004", "1000005"]),
    "1000002": item("1000002"),
    "1000003": collection("1000003", ["1000006", "1000001", "1000004"]),
    "1000004": item("1000004"),
    "1000005": item("1000005"),
    "1000006": item("1000006")
  };
  let inspectedIds;
  const review = await inspectWorkshopManifest(["1000001", "1000002", "1000006"], 322330, {
    ...dependencies(catalog),
    inspect: async (ids) => { inspectedIds = ids; return inventory(["1000004", "1000002"]); }
  });
  assert.deepEqual(review.ids, ["1000001", "1000002", "1000006"]);
  assert.deepEqual(review.contentIds, ["1000006", "1000004", "1000005", "1000002"]);
  assert.deepEqual(inspectedIds, review.contentIds);
  assert.deepEqual(review.installedIds, ["1000004", "1000002"]);
  assert.deepEqual(review.missingIds, ["1000006", "1000005"]);
  assert.deepEqual(review.issues, []);
});

test("superseded manifest reviews stop scheduling batches and never inspect stale content", async () => {
  const ids = Array.from({ length: 320 }, (_, index) => String(1000000 + index));
  const calls = [];
  let current = true;
  let inspections = 0;
  const review = inspectWorkshopManifest(ids, 322330, {
    lookup: (batch) => { const gate = deferred(); calls.push({ batch, gate }); return gate.promise; },
    inspect: async () => { inspections += 1; return inventory(); },
    isCurrent: () => current
  });
  const rejection = assert.rejects(review, { name: "AbortError" });
  assert.equal(calls.length, 4);
  current = false;
  for (const call of calls) call.gate.resolve(call.batch.map((id) => item(id)));
  await rejection;
  assert.equal(calls.length, 4);
  assert.equal(inspections, 0);
});

test("manifest errors propagate and stop new lookup batches", async () => {
  const failure = new Error("lookup unavailable");
  const calls = [];
  const review = inspectWorkshopManifest(Array.from({ length: 320 }, (_, index) => String(1000000 + index)), 322330, {
    lookup: (batch) => { const gate = deferred(); calls.push({ batch, gate }); return gate.promise; },
    inspect: async () => { throw new Error("unexpected inspect"); }, isCurrent: () => true
  });
  const rejection = assert.rejects(review, (error) => error === failure);
  calls[0].gate.reject(failure);
  await rejection;
  for (const call of calls.slice(1)) call.gate.resolve(call.batch.map((id) => item(id)));
  await new Promise(setImmediate);
  assert.equal(calls.length, 4);
  const inspectFailure = new Error("inventory unavailable");
  await assert.rejects(inspectWorkshopManifest(["1000001"], 322330, {
    ...dependencies({ "1000001": item("1000001") }),
    inspect: async () => { throw inspectFailure; }
  }), (error) => error === inspectFailure);
});

test("manifest review rejects incomplete collections, invalid input and wrong inventory app", async () => {
  const catalog = {
    "1000001": item("1000001", { item_kind: "collection", child_count: 2, children: [{ id: "1000002" }] }),
    "1000002": item("1000002")
  };
  const review = await inspectWorkshopManifest(["1000001"], 322330, dependencies(catalog));
  assert.deepEqual(review.issues, [{ id: "1000001", reason: "unresolved" }]);
  await assert.rejects(inspectWorkshopManifest(["invalid"], 322330, dependencies({})), RangeError);
  await assert.rejects(inspectWorkshopManifest(["1000002"], 0, dependencies(catalog)), RangeError);
  await assert.rejects(inspectWorkshopManifest(["1000002"], 322330, {
    ...dependencies(catalog), inspect: async () => inventory([], 108600)
  }), /different Steam app/);
});

test("manifest collection expansion cannot exceed the item or depth budgets", async () => {
  const manyChildren = Array.from({ length: 8192 }, (_, index) => ({ id: String(2000000 + index) }));
  await assert.rejects(inspectWorkshopManifest(["1000001"], 322330, dependencies({
    "1000001": item("1000001", { item_kind: "collection", children: manyChildren, child_count: manyChildren.length })
  })), /8192-item/);
  let calls = 0;
  await assert.rejects(inspectWorkshopManifest(["1000001"], 322330, {
    lookup: async ([id]) => {
      calls += 1;
      return [item(id, { item_kind: "collection", child_count: 1, children: [{ id: String(Number(id) + 1) }] })];
    },
    inspect: async () => { throw new Error("unexpected inspect"); }, isCurrent: () => true
  }), /32-level/);
  assert.equal(calls, 32);
});
