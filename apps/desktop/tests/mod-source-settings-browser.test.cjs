const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const output = process.env.LANGAME_MOD_UI_OUTPUT;
const expectedModules = ["dontstarve", "projectzomboid", "unturned", "arksurvivalevolved", "barotrauma", "conanexiles", "palworld", "soulmask", "squad", "terraria",
  "corekeeper", "enshrouded", "humanitz", "minecraft", "necesse", "rust", "sevendaystodie", "sonsoftheforest", "valheim", "vrising", "windrose",
  "abioticfactor", "arksurvivalascended", "astroneer", "satisfactory"];
const cases = [{ width: 1560, height: 900 }, { width: 960, height: 600 }]
  .map((viewport) => ({ viewport, surface: "abiotic", exercise: true }));
if (output) for (const surface of ["sevendaystodie", "workshop", "projectzomboid", "terraria", "dialog"]) {
  cases.push({ viewport: { width: 1560, height: 900 }, surface, exercise: false });
}
for (const { viewport, surface, exercise } of cases) {
  test(`Unified My Mods ${surface} at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const name = `mod-library-${surface}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "mod-source-settings-browser.html", viewport,
      keyboard: true, pointer: true, fixtureCleanup: true, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__mod_source_settings_fixture") { next(); return; }
        response.setHeader("Content-Type", "application/json");
        response.end(JSON.stringify({ surface, exercise }));
      }, screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.surface, surface);
    if (exercise) {
      assert.deepEqual(report.modules, expectedModules);
      for (const key of ["empty_library", "native_membership", "separate_file_inventory", "source_isolation",
        "remove_preserves_settings", "load_order", "advanced_on_demand", "draft_close_guard", "pending_close_guard",
        "save_failure_recovery", "cancel_preserves_settings", "running_guard", "archive_guard", "sideload_guard", "focus_restored",
        "native_help_on_demand", "invalid_native_help"]) assert.equal(report[key], true, key);
    }
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`MOD_SOURCE_SETTINGS_BROWSER ${JSON.stringify({ surface, viewport, checks: report.checks.length,
      writes: report.writes, modules: report.modules?.length, source_calls: report.source_calls, screenshot: output ? name : undefined })}`);
  });
}
