const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`server archive list provides read-only retained-data tabs and recovery at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshotTab = process.env.LANGAME_SERVER_ARCHIVES_SCREENSHOT_TAB ?? "runtime";
    assert.ok(["settings", "runtime", "maintenance"].includes(screenshotTab), "Archive screenshot tab must be a retained-data tab");
    const screenshotLabel = `server-archives-${screenshotTab}`;
    const screenshotPath = process.env.LANGAME_SERVER_ARCHIVES_SCREENSHOT_DIR
      ? path.join(process.env.LANGAME_SERVER_ARCHIVES_SCREENSHOT_DIR, `${screenshotLabel}-${viewport.width}x${viewport.height}.png`) : undefined;
    const report = await runBrowserFixture({ fixturePath: "server-archives-browser.html", viewport, screenshotPath, keyboard: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__server_archive_screenshot_tab") return next();
        response.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" }); response.end(screenshotTab);
      } });
    assert.equal(report.status, "passed", report.error);
    assert.ok(report.checks.length >= 30, "Archive lifecycle acceptance did not finish");
    assert.deepEqual(report.stable_layout_phases, ["initial load failure", "archive switch loading", "archive switch complete",
      "archive selected", "archive refresh failure", "normal list restored", "failed deletion selected", "failed deletion with empty workspace",
      "archive only switch", "archive only search", "archive only refresh loading", "archive only refresh failure"]);
    assert.deepEqual(report.feedback_phases, ["busy error", "dismissal", "retry", "success", "external warning", "unmount", "inventory warning"]);
    assert.deepEqual(report.concurrency_phases, ["StrictMode shared read", "rapid switches", "mutation switches", "remount shared read"]);
    assert.deepEqual(report.preview_phases, ["loading", "module loading disclosure", "read-only saved values", "unique unknown field navigation", "unavailable states", "blocked recovery preview",
      "error retry", "module fallback", "stale selection", "normal workspace restored", "restored archive removed", "purged archive removed"]);
    assert.deepEqual(report.workspace_phases, ["shared tabs and archive capabilities", "tab keyboard navigation", "retained runtime",
      "saved player access", "saved maintenance and backups", "saved mods and disabled tools", "archive data isolation and no live writes"]);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    assert.equal(report.screenshot_tab, screenshotTab);
    console.log(`SERVER_ARCHIVES_BROWSER ${JSON.stringify({ viewport, status: report.status, checks: report.checks.length, screenshotPath, screenshotTab })}`);
  });
}
