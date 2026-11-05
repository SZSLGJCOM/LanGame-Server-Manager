const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const scenario of [
  { locale: "en-US", viewport: { width: 1560, height: 900 } },
  { locale: "zh-CN", viewport: { width: 960, height: 600 } },
]) {
  test(`Workshop downloads stay visible across navigation in ${scenario.locale} at ${scenario.viewport.width}x${scenario.viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_WORKSHOP_DOWNLOAD_PROGRESS_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const suffix = `${scenario.locale}-${scenario.viewport.width}x${scenario.viewport.height}`;
    const report = await runBrowserFixture({
      fixturePath: "workshop-download-progress-browser.html",
      viewport: scenario.viewport,
      bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__workshop_download_progress_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale: scenario.locale }));
      },
      screenshotPath: output ? path.join(output, `workshop-download-progress-${suffix}.png`) : undefined,
    });
    assert.equal(report.status, "passed");
    assert.equal(report.locale, scenario.locale);
    assert.equal(report.coordinator_count, 1);
    assert.equal(report.writes, 1);
    assert.deepEqual(report.downloads, [
      { instance_id: "fixture-workshop-progress-a", ids: ["1111111"] },
      { instance_id: "fixture-workshop-progress-a", ids: ["2222222"] },
    ]);
    for (const key of ["pending_visible_before_job", "page_progress_restored", "remount_progress_restored",
      "instance_progress_isolated", "duplicate_download_blocked", "completion_deduplicated", "failure_preserved", "fresh_inventory_authoritative"]) {
      assert.equal(report[key], true, key);
    }
    assert.equal(report.document_overflow_x, false);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    if (output) await fs.writeFile(path.join(output, `workshop-download-progress-${suffix}.json`), JSON.stringify(report, null, 2));
    console.log(`WORKSHOP_DOWNLOAD_PROGRESS_BROWSER ${JSON.stringify(report)}`);
  });
}
