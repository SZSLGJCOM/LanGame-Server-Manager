const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const locale of ["zh-CN", "en-US"]) for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Palworld Workshop key failures preserve local recovery and diagnostics in ${locale} at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_WORKSHOP_DOWNLOAD_ERROR_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const suffix = `${locale}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "workshop-download-error-browser.html", viewport,
      bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__workshop_download_error_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale }));
      },
      screenshotPath: output ? path.join(output, `workshop-download-error-${suffix}.png`) : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.locale, locale);
    assert.equal(report.writes, 0);
    assert.deepEqual(report.downloads, [["3816899969"], ["3816899969"]]);
    assert.equal(report.instance_status, "Stopped");
    assert.equal(report.local_import_calls, 1);
    assert.ok(report.layout.dropzone.height >= 120);
    assert.ok(report.layout.configuration.height >= 120);
    assert.equal(report.layout.document_overflow_x, false);
    assert.equal(report.layout.activity_overflow_x, false);
    for (const key of ["original_diagnostic_preserved", "retry_recovered", "local_import_available"]) assert.equal(report[key], true, key);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    if (output) await fs.writeFile(path.join(output, `workshop-download-error-${suffix}.json`), JSON.stringify(report, null, 2));
    console.log(`WORKSHOP_DOWNLOAD_ERROR_BROWSER ${JSON.stringify(report)}`);
  });
}
