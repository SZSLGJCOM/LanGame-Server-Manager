const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`ARK creature spawning interaction at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "ark-creature-spawner-browser.html", viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `ark-creature-spawner-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.clipboard_calls, 0);
    assert.equal(report.spawn_calls, 6);
    assert.ok(report.checks >= 20, "Spawn, identity verification and lifecycle checks must complete");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ARK_CREATURE_SPAWNER_BROWSER ${JSON.stringify(report)}`);
  });
}
