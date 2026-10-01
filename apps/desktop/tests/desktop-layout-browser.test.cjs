const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const scenarios = [
  { width: 1560, height: 900, scale: 1 },
  { width: 1280, height: 720, scale: 1 },
  { width: 960, height: 600, scale: 1 },
  { width: 960, height: 520, scale: 2 },
  { width: 853, height: 453, scale: 1.5 },
  { width: 1920, height: 1080, scale: 1 },
  { width: 1560, height: 900, scale: 1.25 },
  { width: 1560, height: 900, scale: 1.5 },
  { width: 1560, height: 900, scale: 2 },
];
let typographyBaseline;

for (const { width, height, scale } of scenarios) {
  test(`desktop layout remains usable at ${width} x ${height} CSS pixels and ${scale} DPR`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "desktop-layout-browser.html", keyboard: true,
      viewport: { width, height }, deviceScaleFactor: scale,
      fontSelector: ".server-list-result-count",
      screenshotPath: process.env.LANGAME_LAYOUT_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_LAYOUT_SCREENSHOT_DIR, `desktop-layout-${width}x${height}-${scale}.png`) : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.device_scale_factor, scale);
    assert.deepEqual(report.viewport, { width, height });
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.platform_fonts.some((font) => font.isCustomFont && /^Inter/.test(font.familyName) && font.glyphCount > 0),
      "Rendered interface digits must use the bundled Inter face, not a system fallback");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    assert.ok(report.checks >= 12, "The complete layout and interaction sequence must run");
    typographyBaseline ??= report.typography;
    assert.deepEqual(report.typography, typographyBaseline, "Text roles must keep the same CSS size and font at every viewport and DPR");
    console.log(`DESKTOP_LAYOUT_BROWSER ${JSON.stringify(report)}`);
  });
}
