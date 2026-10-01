const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Enshrouded player editor preserves exact native account hashes and rejects invalid input", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "enshrouded-player-access-browser.html",
    viewport: { width: 1280, height: 900 } });
  assert.equal(report.status, "passed");
  assert.ok(report.checks.includes("Remount reads every saved hash without rounding"));
  assert.ok(report.checks.includes("Chinese validation rejects overflow before persistence"));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ENSHROUDED_PLAYER_ACCESS_BROWSER ${JSON.stringify(report)}`);
});
