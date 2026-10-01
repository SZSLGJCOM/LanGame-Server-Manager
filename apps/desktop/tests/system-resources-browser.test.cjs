const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const [width, height, locale, theme, captureScenario] of [
  [1560, 900, "zh-CN", "dark", "normal"],
  [1280, 720, "en-US", "light", "diskPressure"],
  [1280, 720, "en-US", "light", "staleUpdating"]
]) {
  test(`system resources states and interaction at ${width}x${height}, ${locale}, ${theme}`,
    { timeout: 90000 }, async () => {
      const screenshot = process.env.LANGAME_SYSTEM_RESOURCES_SCREENSHOT;
      const screenshotPath = screenshot ? path.join(path.dirname(screenshot),
        `${path.basename(screenshot, path.extname(screenshot))}-${width}-${locale}-${theme}-${captureScenario}.png`) : undefined;
      const report = await runBrowserFixture({ fixturePath: "system-resources-browser.html", keyboard: true,
        viewport: { width, height }, screenshotPath,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__system_resources_config") return next();
          response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, theme, captureScenario }));
        }
      });
      assert.equal(report.status, "passed", report.error);
      assert.equal(report.checks, 15);
      assert.equal(report.locale, locale);
      assert.equal(report.theme, theme);
      assert.deepEqual(report.viewport, { width, height });
      assert.deepEqual(Object.keys(report.scenarios), ["normal", "unknown", "stale", "staleUpdating", "unknownUpdating", "freshUpdating", "memoryPressure", "diskPressure", "diskWatch", "networkWithoutLink", "hardwareFallback", "hardwareUnmatched"]);
      assert.deepEqual(report.interaction, { mouse_selection: true, keyboard_selection: true, escape_release: true });
      assert.equal(report.data_source, "synthetic snapshots rendered by production SystemView");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`SYSTEM_RESOURCES_BROWSER ${JSON.stringify(report)}`);
    });
}
