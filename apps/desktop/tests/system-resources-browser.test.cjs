const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const dialSizes = new Map();
const screenshot = process.env.LANGAME_SYSTEM_RESOURCES_SCREENSHOT;
const scenarios = [
  [1560, 900, "zh-CN", "dark", "normal"],
  [1280, 720, "en-US", "light", "diskPressure"],
  [1280, 800, "zh-CN", "dark", "normal"],
  [1920, 1080, "zh-CN", "dark", "normal"],
  [2560, 1440, "zh-CN", "dark", "normal"]
];
if (screenshot) scenarios.push([1280, 720, "en-US", "light", "staleUpdating", true]);

for (const [width, height, locale, theme, captureScenario, captureOnly = false] of scenarios) {
  test(`system resources ${captureOnly ? "capture" : "states and interaction"} at ${width}x${height}, ${locale}, ${theme}`,
    { timeout: 90000 }, async () => {
      const screenshotPath = screenshot ? path.join(path.dirname(screenshot),
        `${path.basename(screenshot, path.extname(screenshot))}-${width}-${locale}-${theme}-${captureScenario}.png`) : undefined;
      const report = await runBrowserFixture({ fixturePath: "system-resources-browser.html", keyboard: true,
        viewport: { width, height }, screenshotPath,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__system_resources_config") return next();
          response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, theme, captureScenario, captureOnly }));
        }
      });
      assert.equal(report.status, "passed", report.error);
      assert.equal(report.checks, captureOnly ? 2 : 15);
      assert.equal(report.locale, locale);
      assert.equal(report.theme, theme);
      assert.deepEqual(report.viewport, { width, height });
      assert.equal(report.captureScenario, captureScenario);
      assert.deepEqual(Object.keys(report.scenarios), captureOnly ? [captureScenario] : ["normal", "unknown", "stale", "staleUpdating", "unknownUpdating", "freshUpdating", "memoryPressure", "diskPressure", "diskWatch", "networkWithoutLink", "hardwareFallback", "hardwareUnmatched"]);
      assert.deepEqual(report.interaction, captureOnly ? null : { mouse_selection: true, keyboard_selection: true, escape_release: true });
      assert.equal(report.data_source, "synthetic snapshots rendered by production SystemView");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      dialSizes.set(`${width}x${height}`, report.layout);
      console.log(`SYSTEM_RESOURCES_BROWSER ${JSON.stringify(report)}`);
    });
}

test("system core dial scales with the available viewport", () => {
  const [small, medium, large] = ["1280x800", "1920x1080", "2560x1440"].map((size) => dialSizes.get(size));
  assert.ok(small && medium && large, "Every scaling viewport must have rendered");
  assert.ok(medium.dial_size > small.dial_size * 1.2 && large.dial_size > medium.dial_size * 1.2,
    `Dial must grow with the viewport: ${JSON.stringify({ small, medium, large })}`);
  assert.ok(large.metric_label_font > small.metric_label_font,
    `Dial typography must grow with the dial: ${JSON.stringify({ small, large })}`);
  assert.ok(large.metric_label_font <= small.metric_label_font * 1.16,
    `Dial typography must stay modest when maximized: ${JSON.stringify({ small, large })}`);
});
