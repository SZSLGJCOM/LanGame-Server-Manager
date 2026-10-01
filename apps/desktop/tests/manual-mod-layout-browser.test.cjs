const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`manual Mod drop targets fill the remaining workspace at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const output = process.env.LANGAME_MANUAL_MOD_LAYOUT_OUTPUT;
      if (output) {
        assert.ok(path.isAbsolute(output), "Layout evidence output must be absolute");
        const relative = path.relative(path.resolve(__dirname, "../../.."), output);
        assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Layout evidence must stay outside the repository");
        await fs.mkdir(output, { recursive: true });
      }
      const name = `${viewport.width}x${viewport.height}`;
      const report = await runBrowserFixture({ fixturePath: "manual-mod-layout-browser.html", viewport, keyboard: true,
        screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
      if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
      console.log(`MANUAL_MOD_LAYOUT ${JSON.stringify({ viewport: report.viewport, checks: report.checks?.length,
        failures: report.failures, measurements: report.measurements, help_measurements: report.help_measurements, writes: report.writes,
        browser_errors: report.browser_errors, browser_exited: report.browser_exited,
        browser_processes_remaining: report.browser_processes_remaining, scratch_removed: report.scratch_removed })}`);
      assert.equal(report.status, "passed", report.error);
      assert.deepEqual(report.failures, [], "Manual Mod layout requirements failed");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
    });
}
