const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};

function loadApi(response) {
  const filename = path.join(__dirname, "../src/api.ts");
  const exports = {};
  const dependencies = {
    "@tauri-apps/api/core": { isTauri: () => true },
    "./locale-preference": {},
    "./storage-management-requests": loadStorageManagementRequests(),
    "./ark-map-availability": require("../src/ark-map-availability.ts"),
    "./api-transport": { shouldUseLanApi: () => false, invokeOrMock: async () => response }
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (name) => { assert.ok(name in dependencies); return dependencies[name]; }
  });
  return exports;
}

test("a host without slot fields still yields a complete save listing", async () => {
  // Shape returned by a desktop host built before save slots existed.
  const api = loadApi({ instance_id: "i", backup_id: null, root_path: "C:/saves", exists: true,
    entries: [{ relative_path: "w.db", is_directory: false, size_bytes: 1, modified_at_unix_ms: 1 }], truncated: false });
  const listing = await api.listInstanceSaveFiles("i", null);
  assert.deepEqual(JSON.parse(JSON.stringify({ slots: listing.slots, links: listing.skipped_links })), { slots: [], links: [] });
  assert.equal(listing.slot_model, "fallback");
  assert.equal(listing.entries.length, 1);

  const sparse = await loadApi({ slots: [{ id: "t:", name: "", kind: "files" }] }).listInstanceSaveFiles("i", "b1");
  assert.equal(sparse.entries.length, 0);
  assert.equal(sparse.backup_id, "b1");
  assert.equal(sparse.slots[0].paths.length, 0);
});

test("save listing views read the normalized client contract only", () => {
  const source = fs.readFileSync(path.join(__dirname, "../src/views/servers/use-save-file-listing.ts"), "utf8");
  assert.match(source, /listInstanceSaveFiles\(/);
  assert.doesNotMatch(source, /invoke(OrMock)?\(/);
});
