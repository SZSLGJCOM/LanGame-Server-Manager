const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Collection member and whole controls preserve instance ownership at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_COLLECTION_CONTROLS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "workshop-collection-controls-browser.html", viewport,
      screenshotPath: output ? path.join(output, `collection-controls-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `collection-controls-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    for (const flag of ["leaf_deduplication", "download_only_owned_disabled", "shared_member_sync", "whole_toggle_one_cas", "member_remove_repair",
      "protected_whole_removal", "remount_preserved", "running_raw_busy_blocked"]) assert.equal(report[flag], true, flag);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`COLLECTION_CONTROLS_BROWSER ${JSON.stringify(report)}`);
  });
}
