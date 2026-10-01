const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
const scenes = ["mods-empty", "collections-empty", "mods-content", "collections-content", "browse-empty", "search-empty", "no-config", "no-options"];
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  for (const scene of scenes) test(`${scene} uses its Mod panel at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_MOD_EMPTY_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const stem = `${scene}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "mod-empty-collections-browser.html", viewport,
      fixtureMiddleware(request, response, next) { if (request.url !== "/__mod_empty_scene") return next(); response.writeHead(200, { "Content-Type": "text/plain" }); response.end(scene); },
      screenshotPath: output ? path.join(output, `${stem}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${stem}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed");
    {
      for (const empty of report.geometry) { assert.ok(Math.abs(empty.delta_x) <= 2, `${empty.text} is horizontally centered`); assert.ok(Math.abs(empty.delta_y) <= 2, `${empty.text} is vertically centered`); }
      for (const action of report.actions) assert.equal(action.fits, true, `${action.label} fits its action row`);
      if (["mods-empty", "collections-empty"].includes(scene)) {
        assert.equal(report.geometry.length, 1, "Owned empty state has one message");
        assert.equal(report.structure.has_frame, true); assert.equal(report.structure.headers.length, 1);
        assert.equal(report.structure.has_settings, false, "Empty library does not duplicate its message in a settings pane");
      }
      if (scene.endsWith("-content")) {
        assert.equal(report.structure.has_frame, true); assert.equal(report.structure.headers.length, 1); assert.equal(report.structure.has_settings, true);
        if (scene === "collections-content") {
          const mod = report.mod_row_reference;
          assert.equal(report.structure.row_height, mod.height, "Collections use the same row height as Mods");
          assert.equal(report.structure.action_rail_width, mod.action_rail_width, "Collections use the same operation rail as Mods");
          assert.equal(report.structure.visible_members, 2, "Collection accordion exposes its saved members");
        }
      }
      if (scene === "browse-empty" || scene === "search-empty") assert.ok(Math.abs(report.structure.pager_bottom_gap) <= 2, "Empty browse pagination stays at the bottom");
    }
    assert.deepEqual(report.browser_errors, []); assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`MOD_EMPTY_COLLECTIONS ${JSON.stringify(report)}`);
  });
}
