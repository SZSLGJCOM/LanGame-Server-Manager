const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 1280, height: 900 }, { width: 960, height: 600 }]) {
  test(`installation verification reports real local work at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_VERIFICATION_PROGRESS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "installation-verification-browser.html", viewport, fixtureCleanup: true,
      screenshotPath: output ? path.join(output, `verification-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `verification-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed");
    assert.equal(report.scenarios, 7);
    assert.equal(report.stops, 1);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
