const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Mod configuration uses global form density at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_MOD_CONFIG_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "mod-config-density-browser.html", viewport, keyboard: true,
      screenshotPath: output ? path.join(output, `mod-config-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `mod-config-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed");
    const measured = report.computed;
    for (const [actual, reference] of [["modInput", "globalInput"], ["modSelect", "globalSelect"], ["modLabel", "globalLabel"], ["reset", "globalButton"]]) {
      assert.equal(measured[actual].fontSize, measured[reference].fontSize, `${actual} font matches its global control`);
    }
    for (const name of ["modInput", "modSelect", "modNumber", "shardSelect", "reset"]) {
      assert.equal(measured[name].height, measured.globalButton.height, `${name} uses the global standard control height`);
      assert.equal(measured[name].lineHeight, measured.globalButton.lineHeight, `${name} uses the global control line height`);
    }
    assert.equal(measured.modLabel.lineHeight, measured.globalLabel.lineHeight, "Mod labels use the global label line height");
    assert.ok(parseFloat(measured.modSelect.paddingRight) > parseFloat(measured.modSelect.paddingLeft), "Mod select leaves extra space for its dropdown arrow");
    assert.ok(parseFloat(measured.shardSelect.paddingRight) > parseFloat(measured.shardSelect.paddingLeft), "Shard select leaves extra space for its dropdown arrow");
    assert.equal(report.last_option_visible, true, "Last Mod option scrolls fully inside its settings pane");
    assert.equal(report.viewport_fits, true, "Long options do not overflow the workspace");
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`MOD_CONFIG_DENSITY ${JSON.stringify(report)}`);
  });
}
