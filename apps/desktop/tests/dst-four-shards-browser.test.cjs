const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("DST four-shard Mods, configuration ownership and CMD tabs work in the desktop browser", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "dst-four-shards-browser.html",
    viewport: { width: 1366, height: 768 }, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_DST_SHARDS_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.cmd_shards, ["master", "caves", "islands", "volcano"]);
  assert.ok(report.checks.includes("Configuration keeps Mod management in the Mods workspace"));
  assert.ok(report.checks.includes("Imported Lua remains authoritative and exposes its enabled Mod"));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.scratch_removed, true);
  console.log(`DST_FOUR_SHARDS_BROWSER ${JSON.stringify(report)}`);
});
