const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`file Mod management handles imported packages at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_MANAGED_MODS_OUTPUT;
    if (output) {
      assert.ok(path.isAbsolute(output), "Evidence output must be absolute");
      const relative = path.relative(path.resolve(__dirname, "../../.."), output);
      assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Evidence must remain outside the repository");
      await fs.mkdir(output, { recursive: true });
    }
    const name = `${viewport.width}x${viewport.height}`;
    let avatarRequests = 0;
    const report = await runBrowserFixture({ fixturePath: "managed-manual-mods-browser.html", viewport, keyboard: true,
      bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url?.split("?")[0] !== "/__managed_mod_avatar.svg") return next();
        avatarRequests += 1;
        response.statusCode = 200;
        response.setHeader("Content-Type", "image/svg+xml");
        response.setHeader("Cache-Control", "no-store");
        response.end('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><rect width="32" height="32" rx="6" fill="#12685f"/><circle cx="16" cy="16" r="9" fill="#8ce3be"/><path d="M12 20V12L16 17L20 12V20" fill="none" stroke="#16352b" stroke-width="2"/></svg>');
      },
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.success_auto_dismiss_ms >= 4800 && report.success_auto_dismiss_ms < 6500);
    assert.ok(report.warning_persistence_ms >= 5000);
    assert.equal(report.success_manual_dismisses, 3);
    assert.ok(avatarRequests > 0, "The controlled avatar is fetched from the fixture server");
    assert.deepEqual(report.presentation, { title: "Readable Mod with artwork", source_name: "thunderstore-Author-ReadableMod",
      package_id: "author-readable-mod", decoded_stages: ["initial", "disabled", "remounted", "enabled", "final"] });
    assert.deepEqual(report.requests.filter((request) => request.packageId === "author-readable-mod").map((request) => request.action),
      ["disable", "enable", "delete"], "Friendly metadata never replaces the package identity used by management");
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`MANAGED_MODS ${JSON.stringify({ viewport, checks: report.checks.length, requests: report.requests.length,
      locale: report.locale, evidence_scope: report.evidence_scope })}`);
  });
}
