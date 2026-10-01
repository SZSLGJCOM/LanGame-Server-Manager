const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
};

const { buildSteamWorkshopEnablementPlan } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "steam-workshop-enablement-plan.ts"
));

function inventoryItem(name, inferredId, overrides = {}) {
  return {
    name,
    path: `D:\\mods\\${name}`,
    item_type: "directory",
    inferred_id: inferredId,
    file_count: 1,
    total_bytes: 1024,
    modified_unix_ms: 1,
    ...overrides
  };
}

test("attributes a single unmatched Workshop item to the complete inventory delta", () => {
  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000001"],
    inventoryBefore: [],
    inventoryAfter: [
      inventoryItem("collection-main", "MainPackage"),
      inventoryItem("collection-child", "ChildPackage"),
      inventoryItem("unidentifiable", null),
      inventoryItem("blank", "   ")
    ]
  });

  assert.deepEqual(plan.inferredIds, ["MainPackage", "ChildPackage"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("includes a requested Workshop item that already existed before installation", () => {
  const cached = inventoryItem("workshop-3100000002", "CachedPackage");
  const unrelated = inventoryItem("unrelated", "UnrelatedPackage");

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000002"],
    inventoryBefore: [cached, unrelated],
    inventoryAfter: [cached, unrelated]
  });

  assert.deepEqual(plan.inferredIds, ["CachedPackage"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("attributes remaining inventory delta only when one downloaded item remains unmatched", () => {
  const existing = inventoryItem("existing", "ExistingPackage");
  const directChild = inventoryItem("3100000003", "DirectChild");
  const discoveredChild = inventoryItem("collection-child", "DiscoveredChild");

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000003", "3100000004"],
    inventoryBefore: [existing],
    inventoryAfter: [existing, discoveredChild, directChild]
  });

  assert.deepEqual(plan.inferredIds, ["DirectChild", "DiscoveredChild"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("keeps multiple unmatched downloads unresolved instead of guessing from inventory delta", () => {
  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000005", "3100000006"],
    inventoryBefore: [inventoryItem("existing", "ExistingPackage")],
    inventoryAfter: [
      inventoryItem("existing", "ExistingPackage"),
      inventoryItem("new-package", "NewPackage")
    ]
  });

  assert.deepEqual(plan.inferredIds, []);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, ["3100000005", "3100000006"]);
});

test("reports a partially resolved multi-item download", () => {
  const direct = inventoryItem("cache-3100000007", "DirectPackage");
  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000007", "3100000008"],
    inventoryBefore: [direct],
    inventoryAfter: [direct]
  });

  assert.deepEqual(plan.inferredIds, ["DirectPackage"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, ["3100000008"]);
});

test("deduplicates ids case-insensitively while preserving download and inventory order", () => {
  const first = inventoryItem("3100000010", "FirstPackage");
  const second = inventoryItem("3100000020", "SecondPackage");
  const duplicate = inventoryItem("collection-duplicate", "firstpackage");
  const child = inventoryItem("collection-child", "CollectionPackage");

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000020", "3100000010", "3100000030", "3100000020"],
    inventoryBefore: [],
    inventoryAfter: [first, second, duplicate, child]
  });

  assert.deepEqual(plan.inferredIds, ["SecondPackage", "FirstPackage", "CollectionPackage"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("matches Workshop ids as numeric tokens instead of partial path substrings", () => {
  const partial = inventoryItem("93100000004", "PartialMatch");
  const exact = inventoryItem("mod-3100000004-cache", "ExactMatch");

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000004", "not-an-id", ""],
    inventoryBefore: [partial, exact],
    inventoryAfter: [partial, exact]
  });

  assert.deepEqual(plan.inferredIds, ["ExactMatch"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("treats an inferred id discovered at an existing path as a new actionable entry", () => {
  const pathOverride = { path: "D:\\mods\\shared", name: "shared" };

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000005"],
    inventoryBefore: [inventoryItem("shared", null, pathOverride)],
    inventoryAfter: [inventoryItem("shared", "ResolvedPackage", pathOverride)]
  });

  assert.deepEqual(plan.inferredIds, ["ResolvedPackage"]);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});

test("does not use a numeric PackageName as direct Workshop identity", () => {
  const cached = inventoryItem("cached-package", "Package3100000007");

  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["3100000007", "3100000008"],
    inventoryBefore: [cached],
    inventoryAfter: [cached]
  });

  assert.deepEqual(plan.inferredIds, []);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, ["3100000007", "3100000008"]);
});

test("does not infer enablement changes without a valid downloaded Workshop item", () => {
  const plan = buildSteamWorkshopEnablementPlan({
    downloadedWorkshopItemIds: ["", "not-an-id"],
    inventoryBefore: [],
    inventoryAfter: [inventoryItem("new-directory", "UnrelatedPackage")]
  });

  assert.deepEqual(plan.inferredIds, []);
  assert.deepEqual(plan.unresolvedWorkshopItemIds, []);
});
