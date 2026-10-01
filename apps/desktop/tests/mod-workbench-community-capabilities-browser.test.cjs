const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Community Mod capabilities for 22 modules at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_COMMUNITY_CAPABILITIES_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const name = `community-capabilities-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "mod-workbench-community-capabilities-browser.html", viewport,
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.modules.length, 22);
    assert.equal(new Set(report.modules.map((entry) => entry.module)).size, 22);
    assert.equal(report.modules.filter((entry) => entry.gate === false).length, 6);
    assert.equal(report.modules.filter((entry) => entry.client_only_explanation).length, 1);
    assert.equal(report.modules.filter((entry) => entry.file_only_controls_absent).length, 14);
    for (const entry of report.modules.filter((entry) => entry.store_library_switch)) {
      for (const flag of ["empty_inventory", "persisted_inventory", "no_steam_controls"]) assert.equal(entry[flag], true, `${entry.module}: ${flag}`);
    }
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`COMMUNITY_CAPABILITIES_BROWSER ${JSON.stringify(report)}`);
  });
}
