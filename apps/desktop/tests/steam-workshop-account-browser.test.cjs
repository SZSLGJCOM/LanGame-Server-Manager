const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const locale of ["zh-CN", "en-US"]) for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  const scenario = locale === "zh-CN" ? viewport.width < 1000 ? "direct" : "token" : viewport.width < 1000 ? "phone" : "email";
  test(`SteamCMD follows the actual ${scenario} authentication branch in ${locale} at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_WORKSHOP_ACCOUNT_OUTPUT;
    if (output) { assert.equal(path.isAbsolute(output), true); await fs.mkdir(output, { recursive: true }); }
    const suffix = `${locale}-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "steam-workshop-account-browser.html", viewport, keyboard: true, bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url !== "/__steam_workshop_account_locale") return next();
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ locale, scenario }));
      }, screenshotPath: output ? path.join(output, `steam-workshop-account-${suffix}.png`) : undefined });
    assert.equal(report.status, "passed");
    assert.equal(report.locale, locale);
    assert.equal(report.writes, 1);
    assert.equal(report.starts, 7);
    assert.equal(report.cancellations, 3);
    assert.deepEqual(report.downloads.map((entry) => entry.command), ["anonymous", "anonymous", "anonymous", "account", "account", "anonymous", "anonymous", "anonymous", "anonymous", "anonymous", "anonymous"]);
    assert.equal(report.server_commands, 0);
    assert.equal(report.native_token_precedence, true);
    const initialSecrets = scenario === "token" ? 3 : scenario === "email" ? 2 : 1;
    assert.equal(report.challenge_scenario, scenario);
    assert.equal(report.initial_secret_submissions, initialSecrets);
    assert.equal(report.secret_submissions, initialSecrets + 1);
    assert.equal(report.automatic_challenge, true);
    assert.equal(report.password_sent_once, true);
    assert.equal(report.rendered_prompt_bound, true);
    assert.deepEqual(report.credential_order, scenario === "token" ? ["ready", "password", "guard_code", "steam_guard"]
      : scenario === "email" ? ["ready", "password", "email_code"] : ["ready", "password"]);
    assert.equal(report.current_status, true);
    assert.equal(report.terminal_placeholder, true);
    for (const key of ["default_anonymous", "secrets_absent", "cross_tab_retained", "failure_retry", "auto_resume", "generic_failures_do_not_login", "authenticated_failure_no_loop", "instance_change_cancelled", "unmount_cancelled", "stale_completion_safe", "expired_session_recoverable", "safe_lifecycle_errors", "secret_draft_transition_clear"]) assert.equal(report[key], true, key);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    if (output) await fs.writeFile(path.join(output, `steam-workshop-account-${suffix}.json`), JSON.stringify(report, null, 2));
    console.log(`STEAM_WORKSHOP_ACCOUNT_BROWSER ${JSON.stringify(report)}`);
  });
}
