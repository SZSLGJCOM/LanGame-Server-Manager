const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

// Minimum window (tauri.conf.json minWidth/minHeight) plus the common 1280 and default 1560 widths.
for (const viewport of [{ width: 960, height: 600 }, { width: 1280, height: 800 }, { width: 1560, height: 900 }]) {
  test(`Players list header keeps tabs and actions on one row at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_PLAYER_TABS_OUTPUT;
    const label = process.env.LANGAME_PLAYER_TABS_LABEL || "after";
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const name = `${label}-players-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "player-center-tabs-header-browser.html", viewport,
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    console.log(`PLAYER_TABS_HEADER ${JSON.stringify({ viewport, status: report.status, failures: report.failures, error: report.error, measurements: report.measurements })}`);
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.failures, [], report.failures.join("; "));
    assert.ok(report.measurements.every((entry) => entry.tab_count === 7 && entry.actions_same_row && entry.tab_rows === 1));
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
