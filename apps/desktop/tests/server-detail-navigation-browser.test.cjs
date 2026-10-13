const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
const workspaceModules = ["ConfigurationWorkspace", "ModWorkbench", "PlayerCenterWorkbench", "GMToolsWorkbench", "ServerMaintenanceWorkspace"];

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`server detail navigation survives instance and archive card changes at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    let failedModRequest = false;
    const workspaceRequests = [];
    const report = await runBrowserFixture({ fixturePath: "server-detail-navigation-browser.html", viewport, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        let url = new URL(request.url, "http://fixture.local");
        if (url.pathname === "/@vite/lazy") {
          // Locked Vite appends this suffix even when the source already has ?module-retry.
          const id = url.searchParams.get("id");
          const root = path.resolve(__dirname, "..").replaceAll("\\", "/") + "/";
          const lazySuffix = "?rolldown-lazy=1";
          assert.ok(id?.startsWith(root) && id.endsWith(lazySuffix), "Locked Vite lazy entry must belong to this fixture root");
          url = new URL("/" + id.slice(root.length, -lazySuffix.length), url);
        }
        if (url.pathname === "/__workspace_requests") {
          response.writeHead(200, { "Content-Type": "application/json" });
          response.end(JSON.stringify(workspaceRequests));
          return;
        }
        if (workspaceModules.some((name) => url.pathname.endsWith(`/${name}.tsx`))) {
          workspaceRequests.push(url.href);
        }
        if (url.pathname === "/src/views/servers/ModWorkbench.tsx" && !url.searchParams.has("module-retry") && !failedModRequest) {
          failedModRequest = true;
          response.writeHead(503, { "Content-Type": "text/plain" });
          response.end("Simulated unavailable workspace chunk");
          return;
        }
        next();
      },
      screenshotPath: process.env.LANGAME_DETAIL_NAVIGATION_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_DETAIL_NAVIGATION_SCREENSHOT_DIR, `server-detail-navigation-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.equal(failedModRequest, true, "The real lazy module request must exercise load failure recovery");
    assert.equal(report.deferred_loading_verified, true);
    for (const name of workspaceModules) {
      assert.ok(workspaceRequests.some((entry) => new URL(entry).pathname.endsWith(`/${name}.tsx`)),
        `${name} must load through its real lazy request instead of being included in the initial bundle`);
    }
    assert.deepEqual(report.normal_tabs, ["runtime", "settings", "mods", "players", "maintenance", "gm"]);
    assert.deepEqual(report.archive_tabs, ["runtime", "settings", "mods", "players", "maintenance"]);
    assert.ok(report.checks >= 45, "Loading, cross-game fallback and independent mode memory must be verified");
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.unexpected_writes, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`SERVER_DETAIL_NAVIGATION_BROWSER ${JSON.stringify({ viewport, checks: report.checks, status: report.status })}`);
  });
}
