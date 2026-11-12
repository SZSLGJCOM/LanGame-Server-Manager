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
  [960, 600, "zh-CN", "dark", "normal", false, true],
  [1920, 1080, "zh-CN", "dark", "normal", false, true],
  [2560, 1440, "zh-CN", "dark", "normal", false, true],
  [2560, 1440, "en-US", "light", "normal", false, true]
];
if (screenshot) scenarios.push([1280, 720, "en-US", "light", "staleUpdating", true]);

for (const [width, height, locale, theme, captureScenario, captureOnly = false, maximized = false] of scenarios) {
  test(`system resources ${captureOnly ? "capture" : "states and interaction"} at ${width}x${height}, ${locale}, ${theme}`,
    { timeout: 90000 }, async () => {
      const screenshotPath = screenshot ? path.join(path.dirname(screenshot),
        `${path.basename(screenshot, path.extname(screenshot))}-${width}-${locale}-${theme}-${captureScenario}.png`) : undefined;
      const report = await runBrowserFixture({ fixturePath: "system-resources-browser.html", keyboard: true,
        viewport: { width, height }, screenshotPath,
        fixtureMiddleware(request, response, next) {
          if (request.url !== "/__system_resources_config") return next();
          response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, theme, captureScenario, captureOnly, maximized }));
        }
      });
      assert.equal(report.status, "passed", report.error);
      assert.equal(report.checks, captureOnly ? 2 : 15);
      assert.equal(report.locale, locale);
      assert.equal(report.theme, theme);
      assert.deepEqual(report.viewport, { width, height });
      assert.equal(report.captureScenario, captureScenario);
      assert.equal(report.maximized, maximized);
      assert.deepEqual(report.window_typography.restored, report.window_typography.windowed);
      assert.deepEqual(Object.keys(report.scenarios), captureOnly ? [captureScenario] : ["normal", "unknown", "stale", "staleUpdating", "unknownUpdating", "freshUpdating", "memoryPressure", "diskPressure", "diskWatch", "networkWithoutLink", "hardwareFallback", "hardwareUnmatched"]);
      assert.deepEqual(report.interaction, captureOnly ? null : { mouse_selection: true, keyboard_selection: true, escape_release: true });
      assert.equal(report.data_source, "synthetic snapshots rendered by production SystemView");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      dialSizes.set(`${width}x${height}-${locale}`, report.layout);
      console.log(`SYSTEM_RESOURCES_BROWSER ${JSON.stringify(report)}`);
    });
}

test("system core dial retains its panel proportion and interface density when maximized", () => {
  const reference = dialSizes.get("1560x900-zh-CN");
  assert.ok(reference, "The normal window must have rendered");
  assert.ok(reference.dial_panel_ratio > 0.76 && reference.dial_panel_ratio < 0.78,
    "The normal window must enlarge the previous 70% panel proportion by 10%");
  let previousSize = 0;
  for (const size of ["1560x900-zh-CN", "1920x1080-zh-CN", "2560x1440-zh-CN"]) {
    const layout = dialSizes.get(size);
    assert.ok(layout, `The ${size} viewport must have rendered`);
    assert.ok(layout.dial_size >= previousSize, `A larger viewport must not shrink the dial: ${size}`);
    assert.ok(Math.abs(layout.dial_panel_ratio - reference.dial_panel_ratio) < 0.003,
      `The dial must occupy the same proportion of its panel in windowed and maximized views: ${JSON.stringify({ size, reference, layout })}`);
    assert.ok(Math.abs(layout.metric_label_font - layout.adjacent_detail_font) < 0.01,
      "Dial labels must match the adjacent text without stacking a second increase");
    assert.ok(Math.abs(layout.metric_value_font - layout.adjacent_value_font) < 0.01,
      "Dial readings must match the shared display role without stacking a second increase");
    assert.ok(layout.metric_width <= 144, "Reading cards must remain compact when the ring grows");
    assert.ok(layout.metric_height <= reference.metric_height * 1.1 + 1, "Larger dial text must keep its reading cards compact");
    const expectedFonts = size === "1560x900-zh-CN"
      ? { metric_label_font: 12, metric_value_font: 28, center_value_font: 22 }
      : { metric_label_font: 14, metric_value_font: 34, center_value_font: 28 };
    for (const [field, expected] of Object.entries(expectedFonts)) {
      assert.ok(Math.abs(layout[field] - expected) < 0.01,
        `The native window state must apply one fixed text step (${field}): ${JSON.stringify({ size, reference, layout })}`);
    }
    for (const field of ["metric_padding", "metric_gap", "center_padding", "center_gap"]) {
      assert.ok(Math.abs(layout[field] - reference[field]) <= 1,
        `Maximizing must retain the text and control density (${field}): ${JSON.stringify({ size, reference, layout })}`);
    }
    for (const field of ["metric_center_x", "metric_center_y"]) {
      assert.ok(Math.abs(layout[field] - reference[field]) < 0.001,
        `Compact cards must stay anchored to their ring sector (${field}): ${JSON.stringify({ size, reference, layout })}`);
    }
    previousSize = layout.dial_size;
  }
  const maximized = dialSizes.get("2560x1440-zh-CN");
  assert.ok(maximized.dial_size > reference.dial_size * 1.2,
    "The dial must grow with its panel instead of stopping at a fixed pixel cap");
  const small = dialSizes.get("1280x800-zh-CN");
  assert.ok(small && small.dial_size < 380 && small.dial_size > 0,
    "A constrained stage must still shrink the dial without clipping it");
});
