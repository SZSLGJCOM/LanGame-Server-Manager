const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const [width, height, locale, theme] of [
  [1560, 900, "zh-CN", "dark"],
  [1280, 720, "en-US", "light"],
  [1100, 720, "en-US", "dark"],
  [960, 600, "zh-CN", "dark"]
]) {
  test(`system instance header and player count states at ${width}x${height}, ${locale}, ${theme}`,
    { timeout: 90000 }, async () => {
      const screenshot = process.env.LANGAME_SYSTEM_PLAYER_COUNTS_SCREENSHOT;
      const screenshotPath = screenshot ? path.join(path.dirname(screenshot),
        `${path.basename(screenshot, path.extname(screenshot))}-${width}-${locale}-${theme}.png`) : undefined;
      const report = await runBrowserFixture({ fixturePath: "system-player-counts-browser.html",
        viewport: { width, height }, screenshotPath,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__system_player_counts_config") return next();
          response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, theme }));
        }
      });
      assert.equal(report.status, "passed", report.error);
      assert.equal(report.checks, 5);
      assert.equal(report.locale, locale);
      assert.equal(report.theme, theme);
      assert.deepEqual(report.viewport, { width, height });
      const coverage = locale === "zh-CN" ? "人数查询覆盖" : "Player queries";
      assert.deepEqual(report.scenarios, {
        unknown: { value: "— / 32", coverage: `${coverage} 0 / 2` },
        partial: { value: "≥7 / 32", coverage: `${coverage} 1 / 2` },
        complete: { value: "0 / 32", coverage: `${coverage} 2 / 2` },
        idle: { value: "0", coverage: `${coverage} 0 / 0` }
      });
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`SYSTEM_PLAYER_COUNTS_BROWSER ${JSON.stringify(report)}`);
    });
}
