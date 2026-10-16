const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Valheim verified platform actions and name-only fallbacks preserve roster and session authority", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "valheim-selected-player-browser.html", viewport: { width: 1280, height: 900 },
    fixtureCleanup: true, screenshotPath: process.env.LANGAME_VALHEIM_SELECTED_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  for (const locale of ["zh-CN", "en-US"]) {
    for (const check of [
      "only the explicit native character name enables the ban action",
      "hovering a disabled action shows its accurate platform identity reason",
      "keyboard focus shows the disabled action reason without enabling the button",
      "the action pane contains no visible stacked reason paragraphs",
      "inline confirmation explains name collisions changes and unverified native enforcement",
      "confirmed character ban preserves exact native spelling and existing platform entries",
      "a stored character-name ban can be removed without platform escalation",
      "removing the character ban preserves the platform ban",
      "a stored platform ban is not mislabeled as a character-name ban",
      "display names and session identifiers never become a ban target",
      "roster operations never send a live command",
      "verified Rowan enables adding the exact native identity to admin_list",
      "verified Birch enables adding the exact native identity to permitted_list",
      "admin_list add dispatches the exact native Rowan account",
      "admin_list remove dispatches the exact native Rowan account",
      "permitted_list add dispatches the exact native Birch account",
      "permitted_list remove dispatches the exact native Birch account",
      "allowlist add help explains entry restrictions last-entry removal and retained bans",
      "allowlist remove help explains entry restrictions last-entry removal and retained bans",
      "allowlist add confirmation reviews the native restriction scope",
      "allowlist remove confirmation reviews the native restriction scope",
      "verified account changes report actual application in the dynamic activity bar",
      "native Rowan remains selected across a refreshed snapshot",
      "native Birch remains selected across a refreshed snapshot",
      "conflicting native identities cannot mutate banned_list or fall back to a name",
      "persistent roster operations cannot fabricate session command authority",
      "enabling account management requires an actually stopped instance",
      "disabled setup exposes its stop requirement in a keyboard-focus help bubble",
      "explicit setup reviews the runtime change data preservation and automatic identity collection",
      "failed preparation remains retryable and reports no false readiness",
      "confirmed setup prepares only the selected stopped instance",
      "setup reports preparation in the dynamic bar without claiming a connected native identity",
      "setup adds one action without stacking explanatory paragraphs in the right pane",
      "preparing account management preserves world rules and all rosters",
      "an authoritative empty native roster does not offer redundant setup"
    ]) assert.ok(report.checks.includes(`${locale}: ${check}`), `Missing ${locale}: ${check}`);
  }
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`VALHEIM_SELECTED_PLAYER_BROWSER ${JSON.stringify({ status: report.status, checks: report.checks.length })}`);
});
