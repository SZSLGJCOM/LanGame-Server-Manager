const assert = require("node:assert/strict");
const test = require("node:test");
const { deferred, instanceActionFixture } = require("./helpers/instance-action-fixture.cjs");

const backup = { backup_id: "backup-a", display_name: null, backup_path: "a/backups/backup-a", file_count: 1 };
const actions = [
  ["createInstanceBackup", "handleCreateBackup", ["a"]],
  ["restoreInstanceBackup", "handleRestoreBackup", ["a", backup.backup_id]],
  ["renameInstanceBackup", "handleRenameBackup", ["a", backup, "Renamed backup"]],
  ["deleteInstanceBackup", "handleDeleteBackup", ["a", backup]]
];

for (const [apiName, actionName, args] of actions) {
  for (const cachedAtStart of [true, false]) {
    test(`${actionName} preserves newer details when initial cache exists: ${cachedAtStart}`, async () => {
      const mutation = deferred(), readEntered = deferred(), read = deferred();
      const original = { summary: { id: "a", name: "Original name" }, settings_json: '{"retention":3}' };
      const latest = { ...original, summary: { ...original.summary, name: "Updated name" }, settings_json: '{"retention":8}' };
      const fixture = instanceActionFixture({ api: {
        [apiName]: () => mutation.promise,
        readInstanceDetails: () => { readEntered.resolve(); return read.promise; }
      } });
      if (cachedAtStart) fixture.state.detailsById.a = original;
      const pending = fixture.actions[actionName](...args);
      if (cachedAtStart) fixture.state.detailsById = { a: latest };
      mutation.resolve({ ...backup, safeguard_backup_id: "safeguard-a" });
      if (!cachedAtStart) {
        await readEntered.promise;
        fixture.state.detailsById = { a: latest };
        read.resolve(original);
      }
      await pending;
      assert.equal(fixture.state.detailsById.a, latest);
      assert.equal(fixture.state.activity.at(-1).tone, undefined);
      assert.equal(fixture.state.calls.filter(call => call.name === apiName).length, 1);
    });
  }
}
