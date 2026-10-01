const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { readProgramUpdatePolicy, mergeProgramUpdatePolicy, supportsProgramUpdates } = require("../src/views/servers/program-update-policy.ts");

test("existing instances update automatically unless their current version is explicitly pinned", () => {
  assert.equal(readProgramUpdatePolicy({}), "automatic");
  assert.equal(readProgramUpdatePolicy({ program_update: {} }), "automatic");
  for (const policy of ["automatic", "pinned"]) assert.equal(readProgramUpdatePolicy({ program_update: { policy } }), policy);
  for (const program_update of [null, false, [], "pinned", { policy: null }, { policy: "latest" }, { policy: true }]) {
    assert.throws(() => readProgramUpdatePolicy({ program_update }), /program_update_invalid/);
  }
});

test("program policy changes preserve concurrent game, network, backup and mod settings", () => {
  const baseline = { server_name: "Original", program_update: { policy: "automatic" } };
  const latest = { ...baseline, server_name: "Updated elsewhere", network: { bind_ip: "127.0.0.2" },
    mods: ["123"], backup: { retention: 7 }, program_update: { policy: "automatic" } };
  const merged = mergeProgramUpdatePolicy(latest, baseline, "pinned");
  assert.deepEqual(merged, { ...latest, program_update: { policy: "pinned" } });
  assert.equal(latest.program_update.policy, "automatic", "Merging must not mutate the received snapshot");
});

test("unknown program policy fields are rejected rather than silently enabling updates", () => {
  for (const program_update of [{ polciy: "pinned" }, { policy: "pinned", retained: true }, { policy: "automatic", unknown: null }]) {
    const settings = { program_update };
    assert.throws(() => readProgramUpdatePolicy(settings), /program_update_invalid/);
    assert.throws(() => mergeProgramUpdatePolicy(settings, {}, "automatic"), /program_update_invalid/);
  }
});

test("stale policy writes cannot remove a concurrent version pin, while identical writes are idempotent", () => {
  const before = { server_name: "Keep", program_update: { policy: "automatic" } };
  const pinned = { ...before, program_update: { policy: "pinned" } };
  assert.throws(() => mergeProgramUpdatePolicy(pinned, before, "automatic"), /program_update_conflict/);
  assert.deepEqual(mergeProgramUpdatePolicy(pinned, before, "pinned"), pinned);
});

test("automatic updates support Steam and Minecraft but exclude whole-package replacement sources", () => {
  const steam = { summary: { steam_app_id: 343050 }, install: { source: "steamcmd" } };
  assert.equal(supportsProgramUpdates(steam), true);
  assert.equal(supportsProgramUpdates({ summary: {}, install: { source: "minecraft_java" } }), true);
  assert.equal(supportsProgramUpdates({ ...steam, install: null }), false);
  assert.equal(supportsProgramUpdates({ summary: {}, install: {} }), false);
  for (const download_url_windows of ["https://example.invalid/server.zip", ""]) {
    assert.equal(supportsProgramUpdates({ ...steam, install: { ...steam.install, download_url_windows } }), false);
  }
});
