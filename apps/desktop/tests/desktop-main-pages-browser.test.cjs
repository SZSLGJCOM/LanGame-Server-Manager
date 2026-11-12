const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const { viewport, locale, scale = 1, maximized = false, pages = ["system", "catalog", "detail"] } of [
  { viewport: { width: 960, height: 600 }, locale: "zh-CN" },
  { viewport: { width: 786, height: 453 }, scale: 1.5, locale: "zh-CN" },
  { viewport: { width: 786, height: 453 }, scale: 1.5, locale: "en-US", pages: ["system"] },
  { viewport: { width: 901, height: 520 }, scale: 2, locale: "zh-CN" },
  { viewport: { width: 1040, height: 600 }, scale: 0.75, locale: "zh-CN", pages: ["system"] },
  { viewport: { width: 1280, height: 720 }, locale: "en-US", pages: ["system"] },
  { viewport: { width: 1560, height: 900 }, locale: "zh-CN" },
  { viewport: { width: 1560, height: 900 }, locale: "en-US" },
  { viewport: { width: 1920, height: 1040 }, locale: "zh-CN" },
  { viewport: { width: 1920, height: 1040 }, locale: "zh-CN", maximized: true, pages: ["catalog"] },
  { viewport: { width: 2560, height: 1392 }, locale: "en-US", maximized: true, pages: ["catalog"] },
  { viewport: { width: 1280, height: 720 }, scale: 1.5, locale: "zh-CN", maximized: true, pages: ["catalog"] },
]) {
  for (const page of pages) {
    test(`${page} page fits ${viewport.width} x ${viewport.height} in ${locale}${maximized ? " maximized" : ""} and retains usable controls`, { timeout: 90000 }, async () => {
      const report = await runBrowserFixture({ fixturePath: "desktop-main-pages-browser.html", viewport, deviceScaleFactor: scale, keyboard: true,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__desktop_layout_page") return next();
          response.writeHead(200, { "Content-Type": "application/json" });
          response.end(JSON.stringify({ page, locale, maximized }));
        },
        screenshotPath: process.env.LANGAME_LAYOUT_SCREENSHOT_DIR
          ? path.join(process.env.LANGAME_LAYOUT_SCREENSHOT_DIR, `desktop-${page}-${viewport.width}x${viewport.height}-${locale}${maximized ? "-maximized" : ""}.png`) : undefined });
      assert.equal(report.status, "passed");
      assert.equal(report.page, page);
      assert.equal(report.locale, locale);
      assert.equal(report.maximized, maximized);
      assert.deepEqual(report.viewport, viewport);
      assert.equal(report.device_scale_factor, scale);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`DESKTOP_MAIN_PAGE_BROWSER ${JSON.stringify(report)}`);
    });
  }
}
