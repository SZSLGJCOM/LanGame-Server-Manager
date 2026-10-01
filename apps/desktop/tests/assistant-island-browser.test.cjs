const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const expectedCases = [
  "loading-surface-continuity", "spring-geometry-and-content", "explicit-close-accessibility",
  "coordinate-toggle-hit-testing", "close-reverse-continuity", "exit-anchor-tracking", "keyboard-focus-and-draft", "outside-pointer-focus",
  "confirmation-ownership", "theme-geometry", "capsule-anchor-offset", "reduced-motion", "viewport-bounds",
  "unmount-cleanup", "browser-errors",
];

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`LAN island preserves geometry and interaction through expansion and return at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotPath = process.env.LANGAME_ISLAND_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_ISLAND_SCREENSHOT_DIR, `assistant-island-${viewport.width}x${viewport.height}.png`)
      : undefined;
    const report = await runBrowserFixture({ fixturePath: "assistant-island-browser.html", keyboard: true, viewport, screenshotPath, fixtureCleanup: true });
    assert.equal(report.status, "passed", JSON.stringify(report));
    assert.deepEqual(report.cases, expectedCases);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ASSISTANT_ISLAND_BROWSER ${JSON.stringify({ viewport, ...report })}`);
  });
}
