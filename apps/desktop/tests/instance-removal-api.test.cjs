const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { loadStorageManagementRequests } = require("./helpers/storage-management-requests.cjs");

test("archive and delete use distinct commands; archiving is host-only", async () => {
  const filename = path.join(__dirname, "../src/api.ts");
  const calls = [];
  let host = true;
  let lan = false;
  const exports = {};
  const dependencies = {
    "@tauri-apps/api/core": { isTauri: () => host },
    "./locale-preference": {},
    "./storage-management-requests": loadStorageManagementRequests(),
    "./api-transport": { shouldUseLanApi: () => lan, invokeOrMock: async (command, args) => {
      calls.push(JSON.parse(JSON.stringify({ command, args })));
      return { instance_id: args.instanceId };
    } }
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (name) => { assert.ok(name in dependencies); return dependencies[name]; }
  });
  await exports.archiveInstance("archive-target");
  await exports.deleteInstance("delete-target");
  assert.deepEqual(calls, [
    { command: "archive_instance_record", args: { instanceId: "archive-target", instance_id: "archive-target" } },
    { command: "delete_instance_record", args: { instanceId: "delete-target", instance_id: "delete-target" } }
  ]);
  host = false;
  lan = true;
  await assert.rejects(exports.archiveInstance("forbidden"), /desktop host/);
  assert.equal(calls.length, 2);
  await exports.deleteInstance("lan-delete");
  assert.equal(calls.at(-1).command, "delete_instance_record", "Existing LAN deletion keeps its own transport contract");
});
