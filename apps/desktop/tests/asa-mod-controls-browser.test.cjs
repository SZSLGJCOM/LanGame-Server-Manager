const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASA reversible membership at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_ASA_CONTROLS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "asa-mod-controls-browser.html", viewport,
      screenshotPath: output ? path.join(output, `asa-controls-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `asa-controls-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    for (const key of ["id_only_roundtrip", "passive_and_combined_modes", "retained_cache_hidden", "precise_file_restore",
      "fresh_state_conflict", "raw_flags_guard", "running_guard", "pending_guard", "save_failure_preserved"]) assert.equal(report[key], true, key);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`ASA_CONTROLS_BROWSER ${JSON.stringify(report)}`);
  });
}
