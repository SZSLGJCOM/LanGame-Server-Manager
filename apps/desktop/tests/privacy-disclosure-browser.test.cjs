const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 960, height: 600 }, { width: 1560, height: 900 }]) {
  test(`LAN header opens the complete privacy page at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "privacy-disclosure-browser.html", keyboard: true, viewport,
      contentSecurityPolicy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'",
      screenshotPath: process.env.LANGAME_PRIVACY_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_PRIVACY_SCREENSHOT_DIR, `privacy-header-page-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", JSON.stringify(report));
    assert.ok(report.checks >= 80);
    assert.equal(report.surface, "privacy");
    assert.equal(report.screenshotLocale, viewport.width < 1000 ? "en-US" : "zh-CN");
    assert.equal(report.sends, 0);
    assert.equal(report.externalRequests, 0);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`PRIVACY_DISCLOSURE_BROWSER ${JSON.stringify(report)}`);
  });
}
