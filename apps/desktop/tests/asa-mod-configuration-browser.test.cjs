const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASA per-Mod INI configuration component at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_ASA_MOD_CONFIGURATION_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "asa-mod-configuration-browser.html", viewport, keyboard: true, fixtureCleanup: true,
      screenshotPath: output ? path.join(output, `asa-mod-configuration-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `asa-mod-configuration-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error); assert.deepEqual(report.browser_errors, []);
    assert.equal(report.native_settings_saved, false); assert.ok(report.checks.length >= 24);
    assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`ASA_MOD_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
  });
}
