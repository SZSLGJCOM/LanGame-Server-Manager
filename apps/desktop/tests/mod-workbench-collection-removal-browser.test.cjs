const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Collection removal preserves shared Mods, cache and options at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const output = process.env.LANGAME_MOD_REMOVAL_OUTPUT;
      if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
      const report = await runBrowserFixture({ fixturePath: "mod-workbench-collection-removal-browser.html", viewport, keyboard: true,
        screenshotPath: output ? path.join(output, `mod-removal-${viewport.width}x${viewport.height}.png`) : undefined });
      assert.equal(report.status, "passed");
      for (const key of ["cancel_no_writes", "native_focus", "shared_protected", "conflict_preserved", "failure_preserved", "busy_guarded",
        "dst_removal", "squad_removal", "cache_and_options_retained"]) assert.equal(report[key], true, key);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      if (output) await fs.writeFile(path.join(output, `mod-removal-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
      console.log(`MOD_COLLECTION_REMOVAL_BROWSER ${JSON.stringify(report)}`);
    });
}
