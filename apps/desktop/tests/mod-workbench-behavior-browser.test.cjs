const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Mod workspace preserves installation intent, direct settings and classified browsing at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const output = process.env.LANGAME_MOD_BEHAVIOR_OUTPUT;
      if (output) {
        assert.equal(path.isAbsolute(output), true, "Screenshot output must be absolute");
        await fs.mkdir(output, { recursive: true });
      }
      const report = await runBrowserFixture({ fixturePath: "mod-workbench-behavior-browser.html", viewport,
        screenshotPath: output ? path.join(output, `mod-behavior-${viewport.width}x${viewport.height}.png`) : undefined });
      assert.equal(report.status, "passed");
      assert.deepEqual(report.inventory_modules, ["minecraft", "valheim", "squad"]);
      assert.equal(report.squad_managed_path, "C:\\Fixture\\squad\\Mods\\123456");
      assert.equal(report.squad_empty_directory_installed, false);
      assert.equal(report.prepare_saves, 0);
      assert.equal(report.prepare_downloads, 1);
      assert.equal(report.collection_completed, true);
      assert.equal(report.classified_browsing, true);
      assert.equal(report.collection_summary_resolved, true);
      assert.equal(report.direct_configuration, true);
      assert.deepEqual(report.response_failure_recovered, ["missing-kind", "wrong-kind", "wrong-app", "invalid-kind"]);
      assert.deepEqual(report.empty_browse_states, [
        "No Mods are available in this Workshop view.",
        "No matching Mods. Try another name or paste a Workshop URL or ID.",
        "No collections are available in this Workshop view.",
        "No matching collections. Try another name or paste a collection URL or ID."
      ]);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      if (output) await fs.writeFile(path.join(output, `mod-behavior-${viewport.width}x${viewport.height}.json`), JSON.stringify(report, null, 2));
      console.log(`MOD_BEHAVIOR_BROWSER ${JSON.stringify(report)}`);
    });
}
