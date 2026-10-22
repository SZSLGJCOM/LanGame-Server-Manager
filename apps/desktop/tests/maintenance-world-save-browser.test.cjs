const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
const saveModules = ["dontstarve", "minecraft", "projectzomboid", "terraria", "unturned", "arksurvivalevolved",
  "arksurvivalascended", "sevendaystodie", "necesse", "rust", "romestead", "theforest", "humanitz", "satisfactory", "astroneer", "valheim", "runescapedragonwilds", "corekeeper", "vrising", "sonsoftheforest", "abioticfactor", "returntomoria", "windrose", "enshrouded", "rimworld", "barotrauma", "soulmask", "nightingale", "conanexiles", "scum", "palworld"];
const startupPreparedModules = ["corekeeper", "vrising", "sonsoftheforest", "abioticfactor", "returntomoria", "windrose", "rimworld", "barotrauma", "nightingale", "conanexiles", "scum", "unturned", "minecraft", "romestead", "necesse", "projectzomboid", "terraria", "sevendaystodie", "arksurvivalevolved"];
for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`maintenance immediate world save at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "maintenance-world-save-browser.html", viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `maintenance-world-save-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.maintenance_modules, saveModules, "Every connected game must submit from maintenance");
    assert.deepEqual(report.card_modules, saveModules, "Every connected game must submit from its card shortcut");
    assert.deepEqual(report.startup_prepared_modules, startupPreparedModules, "Saving needs no separate preparation control");
    assert.deepEqual(report.persistence_scopes, ["rimworld", "barotrauma", "scum"]);
    assert.deepEqual(report.readiness_guards, ["unprepared-run", "changed-run", "late-result", "current-run"]);
    assert.deepEqual(report.configuration_guards, ["rust", "arksurvivalevolved", "sevendaystodie", "palworld"].flatMap((id) =>
      ["disabled", "password", "missing-port", "udp-port"].map((reason) => `${id}:${reason}`))
      .concat(["password", "missing-port", "udp-port"].map((reason) => `astroneer:${reason}`), "sevendaystodie:eac-preserved"));
    assert.ok(report.checks >= 25, "Save feedback and maintenance navigation checks must complete");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`MAINTENANCE_WORLD_SAVE_BROWSER ${JSON.stringify(report)}`);
  });
}
