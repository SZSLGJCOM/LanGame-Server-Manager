const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const { viewport, locale } of [
  { viewport: { width: 960, height: 600 }, locale: "zh-CN" },
  { viewport: { width: 1560, height: 900 }, locale: "zh-CN" },
  { viewport: { width: 1560, height: 900 }, locale: "en-US" },
]) {
  for (const page of ["system", "catalog", "detail"]) {
    test(`${page} page fits ${viewport.width} x ${viewport.height} in ${locale} and retains usable controls`, { timeout: 90000 }, async () => {
      const report = await runBrowserFixture({ fixturePath: "desktop-main-pages-browser.html", viewport, keyboard: true,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__desktop_layout_page") return next();
          response.writeHead(200, { "Content-Type": "application/json" });
          response.end(JSON.stringify({ page, locale }));
        },
        screenshotPath: process.env.LANGAME_LAYOUT_SCREENSHOT_DIR
          ? path.join(process.env.LANGAME_LAYOUT_SCREENSHOT_DIR, `desktop-${page}-${viewport.width}x${viewport.height}-${locale}.png`) : undefined });
      assert.equal(report.status, "passed");
      assert.equal(report.page, page);
      assert.equal(report.locale, locale);
      assert.deepEqual(report.viewport, viewport);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`DESKTOP_MAIN_PAGE_BROWSER ${JSON.stringify(report)}`);
    });
  }
}
