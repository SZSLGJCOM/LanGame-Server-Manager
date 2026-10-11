const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASA manifest workbench at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_ASA_MANIFEST_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "asa-mod-manifest-browser.html", viewport, pointer: true,
      screenshotPath: output ? path.join(output, `asa-manifest-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `asa-manifest-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.native_download_executed, false);
    assert.equal(report.toolbar_paint.after.filter((tab) => tab.selected).length, 1);
    assert.ok(report.toolbar_paint.after.every((tab) => !tab.hovered && tab.running_animations === 0));
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`ASA_MANIFEST_BROWSER ${JSON.stringify(report)}`);
  });
}
test("ASA installed inventory B configuration fits the actual Workbench column", { timeout: 90000 }, async () => {
  const output = process.env.LANGAME_ASA_MANIFEST_OUTPUT;
  if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
  const report = await runBrowserFixture({ fixturePath: "asa-mod-inventory-configuration-browser.html", viewport: { width: 960, height: 600 },
    pointer: true, fixtureCleanup: true, screenshotPath: output ? path.join(output, "asa-inventory-configuration-960x600.png") : undefined });
  if (output) await fs.writeFile(path.join(output, "asa-inventory-configuration-960x600.json"), JSON.stringify(report, null, 2));
  assert.equal(report.status, "passed", report.error); assert.deepEqual(report.browser_errors, []);
  assert.equal(report.screenshot_view, "inventory-configuration"); assert.equal(report.configuration_layout.selected_mod, "1254808");
  assert.equal(report.toolbar_paint.after.filter((tab) => tab.selected).length, 1);
  assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
  console.log(`ASA_INVENTORY_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
});
