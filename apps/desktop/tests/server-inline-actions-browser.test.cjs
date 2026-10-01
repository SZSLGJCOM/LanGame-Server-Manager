const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const action of ["delete", "archive"]) {
for (const reducedMotion of ["no-preference", "reduce"]) {
test(`real server cards and backup rows confirm ${action} inline without clipping or native dialogs with ${reducedMotion} motion`, { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: action === "archive" ? "server-archive-actions-browser.html" : "server-inline-actions-browser.html", keyboard: true,
    viewport: { width: 800, height: 600 }, reducedMotion,
    screenshotPath: reducedMotion === "no-preference"
      ? action === "archive" ? process.env.LANGAME_SERVER_ARCHIVE_SCREENSHOT : process.env.LANGAME_SERVER_INLINE_SCREENSHOT : undefined });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.reduced_motion, reducedMotion === "reduce");
  assert.equal(report.checks, 10);
  assert.equal(report.native_dialogs, 0);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`SERVER_INLINE_BROWSER ${JSON.stringify(report)}`);
});
}
}
