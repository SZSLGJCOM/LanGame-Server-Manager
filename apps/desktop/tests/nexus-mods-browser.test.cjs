const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Nexus credentials, explicit files and installation at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_NEXUS_MODS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const name = `nexus-install-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "nexus-mods-browser.html", viewport, keyboard: true, pointer: true,
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []); assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(JSON.stringify({ viewport, checks: report.checks?.length, installs: report.installs, credential_writes: report.credential_writes }));
  });
}
