const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Workshop metadata follows locale without stale responses at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_WORKSHOP_LOCALE_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "workshop-locale-browser.html", viewport,
      screenshotPath: output ? path.join(output, `workshop-locale-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed");
    for (const flag of ["stale_requests_ignored", "parent_language_preserved", "author_text_preserved", "manifest_review_isolated", "pending_install_lock", "latest_validation_preserved",
      "localized_failure_retains_text", "localized_retry_recovers", "unverified_fallback_blocks_download"]) assert.equal(report[flag], true, flag);
    assert.equal(report.writes, 1); assert.equal(report.downloads, 1); assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    if (output) await fs.writeFile(path.join(output, `workshop-locale-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    console.log(`WORKSHOP_LOCALE_BROWSER ${JSON.stringify(report)}`);
  });
}
