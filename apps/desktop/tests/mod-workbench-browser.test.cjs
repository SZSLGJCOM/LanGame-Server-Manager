const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 1366, height: 768 }]) {
  test(`Mod workspace preserves layout and local configuration during Workshop throttling at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const report = await runBrowserFixture({ fixturePath: "mod-workbench-browser.html", viewport,
        screenshotPath: viewport.width === 1560 ? process.env.LANGAME_MOD_NOTICES_SCREENSHOT : undefined });
      assert.equal(report.status, "passed");
      assert.ok(report.checks.includes("Rate-limit warning does not resize or move the local configuration panel"));
      assert.equal(report.downloads, 0);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`MOD_NOTICES_BROWSER ${JSON.stringify(report)}`);
    });
}
