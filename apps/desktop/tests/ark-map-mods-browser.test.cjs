const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ARK per-Mod parameter restore popup action at ${viewport.width}x${viewport.height}`, { timeout: 110000 }, async () => {
    const output = process.env.LANGAME_ARK_MAP_MODS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "ark-map-mods-browser.html", viewport, keyboard: true, pointer: true, fixtureCleanup: true, fixtureTimeoutMs: 55000,
      screenshotPath: output ? path.join(output, `ark-mod-restore-action-final-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `ark-mod-restore-action-final-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.checks.length >= 60);
    assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`ARK_MAP_MODS_BROWSER ${JSON.stringify(report)}`);
  });
}
