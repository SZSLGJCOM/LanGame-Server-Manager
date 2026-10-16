const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`file Mod management handles imported packages at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_MANAGED_MODS_OUTPUT;
    if (output) {
      assert.ok(path.isAbsolute(output), "Evidence output must be absolute");
      const relative = path.relative(path.resolve(__dirname, "../../.."), output);
      assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Evidence must remain outside the repository");
      await fs.mkdir(output, { recursive: true });
    }
    const name = `${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "managed-manual-mods-browser.html", viewport, keyboard: true,
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`MANAGED_MODS ${JSON.stringify({ viewport, checks: report.checks.length, requests: report.requests.length,
      locale: report.locale, evidence_scope: report.evidence_scope })}`);
  });
}
