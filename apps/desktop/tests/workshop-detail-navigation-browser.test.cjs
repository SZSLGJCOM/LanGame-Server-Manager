const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Workshop collection members open details and return at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_WORKSHOP_DETAIL_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "workshop-detail-navigation-browser.html", viewport, keyboard: true,
      screenshotPath: output ? path.join(output, `workshop-navigation-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed");
    for (const flag of ["keyboard_member_open", "unknown_member_retry", "nested_back", "client_members_filtered", "all_client_empty", "supported_sorting",
      "atomic_detail_loading", "cached_detail_stable", "local_error_retry"]) assert.equal(report[flag], true, flag);
    assert.equal(report.writes, 0); assert.equal(report.downloads, 0); assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    if (output) await fs.writeFile(path.join(output, `workshop-navigation-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    console.log(`WORKSHOP_DETAIL_NAVIGATION ${JSON.stringify(report)}`);
  });
}
