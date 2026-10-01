const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const games = ["projectzomboid", "palworld", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask", "unturned", "terraria", "squad"];
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Workshop cross-game collection controls at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_CROSS_GAME_CONTROLS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const report = await runBrowserFixture({ fixturePath: "workshop-cross-game-controls-browser.html", viewport,
      screenshotPath: output ? path.join(output, `cross-game-controls-${viewport.width}x${viewport.height}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `cross-game-controls-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.games.map((entry) => entry.module), games);
    for (const entry of report.games) {
      assert.equal(entry.reversible, !["unturned", "terraria", "squad"].includes(entry.module));
      for (const flag of ["single_remove_repair", "protected_whole_removal", "leaf_visibility_and_remount"]) assert.equal(entry[flag], true, `${entry.module}: ${flag}`);
    }
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`CROSS_GAME_CONTROLS_BROWSER ${JSON.stringify(report)}`);
  });
}
