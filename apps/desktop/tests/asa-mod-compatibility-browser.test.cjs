const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASA platforms live only in Configuration at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_ASA_COMPATIBILITY_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "asa-mod-compatibility-browser.html", viewport, pointer: true,
      screenshotPath: output ? path.join(output, `asa-compatibility-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `asa-compatibility-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    for (const key of ["configuration_only", "help_on_demand", "linked_platform_save", "start_barrier", "preserves_mods", "custom_platforms",
      "conflict_guard", "running_guard", "raw_flags_guard", "failure_visible", "layout_visible"]) assert.equal(report[key], true, key);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ASA_COMPATIBILITY_BROWSER ${JSON.stringify(report)}`);
  });
}
