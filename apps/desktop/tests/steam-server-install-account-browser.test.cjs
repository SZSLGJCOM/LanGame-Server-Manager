const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const locale of ["zh-CN", "en-US"]) for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  const scenario = locale === "zh-CN" ? viewport.width < 1000 ? "phone" : "email" : viewport.width < 1000 ? "direct" : "token";
  test(`Steam server installation resumes its original account scope in ${locale} at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_STEAM_SERVER_ACCOUNT_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const suffix = `${locale}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "steam-server-install-account-browser.html", viewport, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__steam_server_account_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, scenario }));
      }, screenshotPath: output ? path.join(output, `steam-server-install-account-${suffix}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `steam-server-install-account-${suffix}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.locale, locale);
    assert.equal(report.mutations, 0);
    assert.equal(report.native_token_precedence, true);
    assert.equal(report.challenge_scenario, scenario);
    assert.equal(report.cached_login, scenario === "direct");
    assert.equal(report.automatic_challenge, true);
    assert.equal(report.password_not_replayed, true);
    assert.equal(report.current_status, true);
    for (const key of ["library_no_instance", "instance_install", "maintenance_update", "no_loop", "cancellation", "stale_owner", "anonymous_revocation", "secrets_absent"]) assert.equal(report[key], true, key);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true); assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(`STEAM_SERVER_ACCOUNT_BROWSER ${JSON.stringify(report)}`);
  });
}
for (const locale of ["zh-CN", "en-US"]) for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  const scenario = locale === "zh-CN" ? viewport.width < 1000 ? "phone" : "email" : viewport.width < 1000 ? "direct" : "token";
  test(`Library first-download LanGameCMD is visible in ${locale} at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_STEAM_SERVER_ACCOUNT_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const suffix = `${locale}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "steam-server-install-account-browser.html", viewport, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__steam_server_account_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, scenario, captureLibrary: true }));
      }, screenshotPath: output ? path.join(output, `steam-library-account-${suffix}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `steam-library-account-${suffix}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error); assert.equal(report.scope, "module"); assert.equal(report.library_no_instance, true);
    assert.equal(report.native_token_precedence, true);
    assert.equal(report.challenge_scenario, scenario);
    assert.equal(report.cached_login, scenario === "direct");
    assert.equal(report.automatic_challenge, true);
    assert.equal(report.current_status, true);
    assert.deepEqual(report.browser_errors, []); assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
  });
}
