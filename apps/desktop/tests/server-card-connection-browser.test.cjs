const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("real instance cards show saved ports and copy fresh join addresses without changing card layout", { timeout: 180000 }, async () => {
  for (const width of [1560, 960]) {
    const screenshot = process.env.LANGAME_SERVER_CONNECTION_SCREENSHOT;
    const screenshotPath = screenshot ? path.join(path.dirname(screenshot), `${path.basename(screenshot, path.extname(screenshot))}-${width}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "server-card-connection-browser.html", keyboard: true,
      viewport: { width, height: 900 }, screenshotPath });
    assert.equal(report.status, "passed");
    assert.equal(report.checks, 12);
    assert.equal(report.card_height, 194);
    assert.equal(report.card_shell_height, 192);
    assert.equal(report.start_width, 88);
    assert.equal(report.native_dialogs, 0);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`SERVER_CONNECTION_BROWSER ${JSON.stringify(report)}`);
  }
});
