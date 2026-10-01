const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

// Model assertions do not render styles; the real-browser suite covers layout.
require.extensions[".css"] = () => {};
require.extensions[".tsx"] = require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const {
  formatWorkshopByteSize,
  resolveWorkshopInstallationState,
  resolveWorkshopLifecycleState,
  resolveWorkshopStoreItemState,
  workshopDescriptionText
} = require(path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "steam-workshop-store-model.ts"
));

function workshopItem(overrides = {}) {
  return {
    id: "1000001",
    title: "Test Mod",
    preview_url: null,
    description_excerpt: null,
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=1000001",
    item_kind: "item",
    status: "resolved",
    message: null,
    consumer_app_id: 108600,
    creator_app_id: 108600,
    child_count: 0,
    children: [],
    ...overrides
  };
}

test("collection installation state reflects all expanded child items", () => {
  const collection = workshopItem({
    item_kind: "collection",
    child_count: 2,
    children: [{ id: "1000002", item_kind: "item", status: "resolved" }, { id: "1000003", item_kind: "item", status: "resolved" }]
  });
  const inspected = new Set(["1000002", "1000003"]);

  assert.equal(resolveWorkshopInstallationState(collection, new Set(["1000002"]), inspected, false), "partial");
  assert.equal(resolveWorkshopInstallationState(collection, new Set(["1000002", "1000003"]), inspected, false), "installed");
});

function storeState(item, overrides = {}) {
  const contentIds = item.item_kind === "collection" ? item.children.map((child) => child.id) : [item.id];
  return resolveWorkshopStoreItemState({
    item, moduleId: "dontstarve", expectedAppId: 322330,
    configuredIds: new Set(), cachedIds: new Set(contentIds), inspectedIds: new Set(contentIds),
    inspectionFailed: false, installingIds: new Set(), steamDownloadMode: "steamcmd-cache", ...overrides
  });
}

test("cached collections retain an install action until every member is configured", () => {
  const item = workshopItem({
    item_kind: "collection", consumer_app_id: 322330, child_count: 2,
    children: ["1000002", "1000003"].map((id) => ({ id, item_kind: "item", status: "resolved" }))
  });
  const partial = storeState(item, { configuredIds: new Set(["1000002"]) });
  assert.equal(partial.action, "install", "The remaining cached member must still be addable");
  assert.equal(partial.configured, false);
  assert.equal(partial.enabled, false);
  assert.equal(partial.lifecycleState, "partially-configured");

  const whole = storeState(item, { configuredIds: new Set(["1000002", "1000003"]) });
  assert.equal(whole.action, "manage");
  assert.equal(whole.lifecycleState, "enabled");
  assert.equal(storeState(item, { configuredIds: new Set([item.id]) }).action, "install",
    "A saved collection reference alone does not enable its child Mods");
});

test("DST collections become manageable when all server members are installed even if client Mods were skipped", () => {
  const server = { id: "1000002", item_kind: "item", status: "resolved", consumer_app_id: 322330 };
  const client = { ...server, id: "1365141672", tags: ["CLIENT_ONLY_MOD"] };
  const collection = workshopItem({ item_kind: "collection", consumer_app_id: 322330,
    child_count: 2, children: [server, client] });
  const state = storeState(collection, { configuredIds: new Set([server.id]), cachedIds: new Set([server.id]),
    inspectedIds: new Set([server.id]) });
  assert.equal(state.installationState, "installed");
  assert.equal(state.configured, true);
  assert.equal(state.lifecycleState, "enabled");
  assert.equal(state.action, "manage");
  const allClient = { ...collection, child_count: 1, children: [client] };
  const empty = storeState(allClient, { configuredIds: new Set([client.id]) });
  assert.equal(empty.configured, false);
  assert.equal(empty.enabled, false);
  assert.equal(empty.lifecycleState, "client-only");
  const { buildDownloadableSteamIds } = require("../src/views/servers/mod-workbench-model.ts");
  assert.deepEqual(buildDownloadableSteamIds([allClient.id], { [allClient.id]: allClient }, 322330), []);
  const foreign = { ...collection, children: [server, { ...client, consumer_app_id: 108600 }] };
  assert.equal(storeState(foreign, { configuredIds: new Set([server.id]), cachedIds: new Set([server.id]) }).configured, false,
    "A wrong-game client tag cannot be silently discarded for an ownership claim");
});

test("store actions preserve missing-file repair, PZ activation and unsupported item states", () => {
  const item = workshopItem({ consumer_app_id: 322330 });
  const configuredIds = new Set([item.id]);
  const pending = storeState(item, { configuredIds, cachedIds: new Set() });
  assert.equal(pending.action, "install");
  assert.equal(pending.lifecycleState, "pending-download");
  const pz = storeState(item, { moduleId: "projectzomboid", configuredIds, enabled: false });
  assert.equal(pz.action, "install");
  assert.equal(pz.lifecycleState, "needs-configuration");
  const client = storeState({ ...item, tags: ["client_only_mod"] });
  assert.equal(client.lifecycleState, "client-only");
  const busy = storeState(item, { installingIds: new Set([item.id]) });
  assert.equal(busy.lifecycleState, "installing");
});

test("PZ active internal IDs cannot replace missing Workshop membership", () => {
  const item = workshopItem();
  const options = { moduleId: "projectzomboid", expectedAppId: 108600, enabled: true };
  const missing = storeState(item, options);
  assert.equal(missing.configured, false);
  assert.equal(missing.enabled, false);
  assert.equal(missing.action, "install", "Cached internal Mod IDs still need their WorkshopItems entry");
  assert.equal(missing.lifecycleState, "downloaded");

  const collection = workshopItem({ item_kind: "collection", child_count: 2,
    children: ["1000002", "1000003"].map((id) => ({ id, item_kind: "item", status: "resolved" })) });
  const partial = storeState(collection, { ...options, configuredIds: new Set(["1000002"]) });
  assert.equal(partial.enabled, false);
  assert.equal(partial.action, "install", "Every active collection member also needs Workshop membership");
  assert.equal(partial.lifecycleState, "partially-configured");
  const complete = storeState(collection, { ...options, configuredIds: new Set(["1000002", "1000003"]) });
  assert.equal(complete.enabled, true);
  assert.equal(complete.action, "manage");
});

test("lifecycle distinguishes downloaded files from active server configuration", () => {
  assert.equal(resolveWorkshopLifecycleState({
    installationState: "installed",
    configured: false,
    enabled: false,
    installing: false,
    moduleId: "projectzomboid"
  }), "downloaded");
  assert.equal(resolveWorkshopLifecycleState({
    installationState: "installed",
    configured: true,
    enabled: false,
    installing: false,
    moduleId: "projectzomboid"
  }), "needs-configuration");
  assert.equal(resolveWorkshopLifecycleState({
    installationState: "not-installed",
    configured: true,
    enabled: true,
    installing: false,
    moduleId: "dontstarve"
  }), "pending-download");
});

test("Workshop descriptions are rendered as safe plain text", () => {
  assert.equal(
    workshopDescriptionText("[h1]Title[/h1]\n[url=https://example.com]Guide[/url]\n[*] First &amp; second"),
    "Title\nGuide\n• First & second"
  );
});

test("Workshop file sizes use compact binary units", () => {
  assert.equal(formatWorkshopByteSize(1536), "1.50 KB");
  assert.equal(formatWorkshopByteSize(104857600), "100 MB");
  assert.equal(formatWorkshopByteSize(null), "");
});


test("only resolved items and expanded collections can enter the download plan", () => {
  const { buildDownloadableSteamIds } = require("../src/views/servers/mod-workbench-model.ts");
  const map = {
    "1000001": workshopItem(),
    "1000002": workshopItem({ id: "1000002", status: "pending" }),
    "1000003": workshopItem({ id: "1000003", status: "not_found" }),
    "1000004": workshopItem({ id: "1000004", consumer_app_id: 322330 }),
    "1000005": workshopItem({ id: "1000005", item_kind: "collection", children: [] }),
    "1000006": workshopItem({ id: "1000006", item_kind: "collection", children: [{ id: "1000007", item_kind: "item", status: "resolved", consumer_app_id: 108600 }] }),
    "1000009": workshopItem({ id: "1000009", item_kind: "guide", status: "unsupported" }),
    "1000010": workshopItem({ id: "1000010", item_kind: "unsupported", status: "unsupported" })
  };
  assert.deepEqual(buildDownloadableSteamIds([...Object.keys(map), "1000008"], map, 108600), ["1000001", "1000007"]);
});
