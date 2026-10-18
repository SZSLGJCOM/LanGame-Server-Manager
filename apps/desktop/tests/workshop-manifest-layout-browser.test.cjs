const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const output = process.env.LANGAME_MANIFEST_LAYOUT_OUTPUT;
const prefix = process.env.LANGAME_MANIFEST_LAYOUT_PREFIX || "manifest";
for (const [fixturePath, view] of [["workshop-manifest-layout-browser.html", "review"], ["workshop-manifest-initial-browser.html", "initial"]]) {
  for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
    test(`Workshop manifest ${view} layout at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
      if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
      const name = `${prefix}-${view}-${viewport.width}x${viewport.height}`;
      const report = await runBrowserFixture({ fixturePath, viewport, pointer: true,
        screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
      if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
      assert.equal(report.status, "passed", report.error);
      assert.equal(report.final_view, view);
      assert.deepEqual(report.browser_errors, []);
      assert.ok(report.layout.document_overflow_x <= 1 && report.layout.panel_overflow_x <= 1);
      assert.equal(report.layout.split, viewport.width >= 1200, "desktop splits panes; narrow viewports stack them");
      assert.equal(report.layout.framed_panes, 0);
      assert.equal(report.layout.visible_help_paragraphs, 0);
      if (view === "review") assert.ok(report.layout.actions && report.layout.actions.bottom <= viewport.height);
      assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
      console.log(`WORKSHOP_MANIFEST_LAYOUT ${JSON.stringify({ view, viewport, checks: report.checks.length, layout: report.layout })}`);
    });
  }
}
