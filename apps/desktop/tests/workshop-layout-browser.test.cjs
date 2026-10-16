const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1366, height: 768 }, { width: 1920, height: 1080 }, { width: 2560, height: 1440 }]) {
  test(`Workshop browsing uses the available height at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const output = process.env.LANGAME_WORKSHOP_LAYOUT_OUTPUT;
      if (output) {
        assert.ok(path.isAbsolute(output), "Layout evidence output must be absolute");
        const relative = path.relative(path.resolve(__dirname, "../../.."), output);
        assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Layout evidence must stay outside the repository");
        await fs.mkdir(output, { recursive: true });
      }
      const name = `${viewport.width}x${viewport.height}`;
      const report = await runBrowserFixture({ fixturePath: "workshop-layout-browser.html", viewport,
        screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
      if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
      console.log(`WORKSHOP_LAYOUT ${JSON.stringify(report)}`);
      assert.equal(report.status, "passed", report.error);
      assert.deepEqual(report.failures, [], "Workshop layout requirements failed");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
    });
}
