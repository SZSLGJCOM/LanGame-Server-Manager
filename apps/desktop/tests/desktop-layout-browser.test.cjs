const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const viewports = [
  { width: 1560, height: 900, scale: 1 },
  { width: 1280, height: 720, scale: 1 },
  // Automatic 1080p fitting retains the same design viewport as the 1440p baseline.
  { width: 1560, height: 900, scale: 0.75 },
  // Lower resolutions use the minimum physical text scale and responsive layout.
  { width: 1040, height: 600, scale: 0.75 },
  { width: 2560, height: 1392, scale: 1 },
  { width: 960, height: 600, scale: 1 },
  { width: 960, height: 520, scale: 2 },
  { width: 853, height: 453, scale: 1.5 },
  { width: 786, height: 453, scale: 1.5 },
  { width: 901, height: 520, scale: 2 },
  { width: 1920, height: 1080, scale: 1 },
  { width: 1560, height: 900, scale: 1.25 },
  { width: 1560, height: 900, scale: 1.5 },
  { width: 1560, height: 900, scale: 2 },
];
const scenarios = [
  ...viewports.map((viewport) => ({ ...viewport, reducedMotion: "no-preference" })),
  { width: 1560, height: 900, scale: 1, reducedMotion: "reduce" },
  { width: 853, height: 453, scale: 1.5, reducedMotion: "reduce" },
  { width: 960, height: 600, scale: 1, reducedMotion: "no-preference", maximized: true },
  { width: 1280, height: 720, scale: 1, reducedMotion: "no-preference", maximized: true },
  { width: 1920, height: 1040, scale: 1, reducedMotion: "no-preference", maximized: true },
  { width: 2560, height: 1392, scale: 1, reducedMotion: "no-preference", maximized: true },
];
const typographyBaselines = new Map();

for (const { width, height, scale, reducedMotion, maximized = false } of scenarios) {
  test(`desktop layout remains usable at ${width} x ${height} CSS pixels and ${scale} DPR with ${reducedMotion} motion${maximized ? " maximized" : ""}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "desktop-layout-browser.html", keyboard: true,
      viewport: { width, height }, deviceScaleFactor: scale, reducedMotion,
      bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__server_layout_state") return next();
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(JSON.stringify({ maximized }));
      },
      fontSelector: ".server-list-result-count",
      screenshotPath: process.env.LANGAME_LAYOUT_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_LAYOUT_SCREENSHOT_DIR, `desktop-layout-${width}x${height}-${scale}-${reducedMotion}${maximized ? "-maximized" : ""}.png`) : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.maximized, maximized);
    assert.equal(report.reduced_motion, reducedMotion === "reduce");
    assert.equal(report.device_scale_factor, scale);
    assert.deepEqual(report.viewport, { width, height });
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.platform_fonts.some((font) => font.isCustomFont && /^Inter/.test(font.familyName) && font.glyphCount > 0),
      "Rendered interface digits must use the bundled Inter face, not a system fallback");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    assert.ok(report.checks >= 12, "The complete layout and interaction sequence must run");
    if (!typographyBaselines.has(maximized)) typographyBaselines.set(maximized, report.typography);
    assert.deepEqual(report.typography, typographyBaselines.get(maximized),
      "Each window state must retain its text sizes and fonts at every viewport and DPR");
    console.log(`DESKTOP_LAYOUT_BROWSER ${JSON.stringify(report)}`);
  });
}
