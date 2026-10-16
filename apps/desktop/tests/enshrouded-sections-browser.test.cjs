const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Enshrouded categories, explicit preset and role settings work at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true, bundleWithMiddleware: true,
      fixtureMiddleware: (request, response, next) => {
        if (request.url !== "/__configuration_review_mode") return next();
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify({ mode: "enshrouded-sections" }));
      },
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `enshrouded-sections-${viewport.width}x${viewport.height}.png`)
        : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.configuration_fields, 74);
    assert.equal(report.gameplay_fields, 38);
    assert.deepEqual(report.custom_requirement_sections, ["players", "survival", "world", "progression", "combat"]);
    assert.equal(report.preset_requires_explicit_edit, true);
    assert.deepEqual(report.minute_value_readbacks, { hunger: 15, day: 45, night: 18 });
    assert.equal(report.role_password_save_readback, true);
    assert.deepEqual(report.room_password_editors, ["admin_password", "friend_password", "guest_password", "visitor_password"]);
    assert.deepEqual(report.password_search_queries, ["房间密码", "服务器密码", "加入密码"]);
    assert.equal(report.exact_bans_preserved, true);
    assert.equal(report.setting_save_readbacks, 8);
    assert.equal(report.save_boundary, "synthetic onSave; no server files written");
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ENSHROUDED_SECTIONS_BROWSER ${JSON.stringify(report)}`);
  });
}
