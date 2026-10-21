const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }, { width: 1366, height: 768 }, { width: 1920, height: 1080 }]) {
  test(`world map native boundary and four shard navigation ${viewport.width}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "dst-world-map-browser.html", viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `world-map-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.checks >= 16);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DST_WORLD_MAP_BROWSER ${JSON.stringify(report)}`);
  });
}
