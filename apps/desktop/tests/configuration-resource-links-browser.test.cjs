const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const width of [960, 1560]) {
  test(`configuration resource titles open official pages without changing secrets at ${width}px`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "configuration-resource-links-browser.html", keyboard: true,
      viewport: { width, height: 900 }, fixtureCleanup: true,
      screenshotPath: process.env[`LANGAME_CONFIGURATION_RESOURCE_LINKS_SCREENSHOT_${width}`]
        ?? (width === 1560 ? process.env.LANGAME_CONFIGURATION_RESOURCE_LINKS_SCREENSHOT : undefined) });
    assert.equal(report.status, "passed");
    assert.equal(report.resource_fields, 4);
    assert.equal(report.open_requests, 10);
    assert.equal(report.successful_opens, 9);
    assert.equal(report.patches, 0);
    assert.equal(report.failure_recovered, true);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`CONFIGURATION_RESOURCE_LINKS_BROWSER ${JSON.stringify(report)}`);
  });
}
