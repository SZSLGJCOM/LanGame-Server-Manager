const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("system instance overview displays saved join endpoints in Chinese and English without changing row actions", { timeout: 180000 }, async () => {
  for (const [width, locale] of [[1560, "zh-CN"], [960, "en-US"]]) {
    const screenshot = process.env.LANGAME_SYSTEM_CONNECTION_SCREENSHOT;
    const screenshotPath = screenshot ? path.join(path.dirname(screenshot), `${path.basename(screenshot, path.extname(screenshot))}-${width}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "system-instance-connections-browser.html", keyboard: true,
      viewport: { width, height: 900 }, screenshotPath, fixtureMiddleware(request, response, next) {
        if (request.url !== "/__system_connection_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale }));
      } });
    assert.equal(report.status, "passed");
    assert.equal(report.checks, 6);
    assert.equal(report.locale, locale);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`SYSTEM_CONNECTION_BROWSER ${JSON.stringify(report)}`);
  }
});
