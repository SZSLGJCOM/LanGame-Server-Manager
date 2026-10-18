const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("read-only players support keyboard inspection and explain unavailable management", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "player-details-browser.html", keyboard: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_PLAYER_DETAILS_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.ok(report.checks.includes("Keyboard selects an Enshrouded name without granting native account authority"));
  assert.ok(report.checks.includes("A game with no player actions still exposes selected details"));
  assert.ok(report.checks.includes("Inspecting expired players never sends live commands"));
  assert.ok(report.checks.includes("Read-only inspection rejects a live command even when the supplied snapshot is actionable"));
  assert.ok(report.checks.includes("Unknown native extension preserves read-only fallback despite declared actions"));
  for (const scenario of ["enshrouded-unsupported-build", "enshrouded-not-installed", "enshrouded-loading", "enshrouded-load-failed", "enshrouded-protocol-incomplete"]) {
    assert.ok(report.checks.includes(`Selected fallback player exposes ${scenario} beside and on disabled actions`));
  }
  assert.ok(report.checks.includes("Keyboard reaches the disabled ban action explanation"));
  assert.ok(report.checks.includes("Hover reveals the extension failure directly on a disabled action"));
  assert.ok(report.checks.includes("Unavailable extensions disable known roster and manual native unban with a specific reason"));
  assert.ok(report.checks.includes("Disabled native unban never crosses the mutation boundary"));
  assert.ok(report.checks.includes("Ready or incomplete player identities and a stopped server do not block known-account unban"));
  assert.ok(report.checks.includes("Available native unban or stopped ban removal sends only the exact known account"));
  assert.ok(report.checks.includes("Steam identity alone never authorizes a native account action"));
  assert.ok(report.checks.includes("Protected native players cannot dispatch moderation commands"));
  assert.ok(report.checks.includes("Native ban dispatch retains snapshot references without substituting Steam or persistent mutations"));
  assert.ok(report.checks.includes("DST live administrator absent from admin_list explains independent permission sources"));
  assert.ok(report.checks.includes("DST empty administrator list cannot send a fake permission removal"));
  assert.ok(report.checks.includes("DST live administrator role alone does not disable its declared kick action"));
  assert.ok(report.checks.includes("DST administrator kick can reach review without sending a command"));
  assert.ok(report.checks.includes("DST live administrator keeps independent persistent roster actions"));
  assert.ok(report.checks.includes("DST administrator roster operations retain the exact selected Klei identity"));
  assert.ok(report.checks.includes("DST listed administrator removal explains restart and other permission sources"));
  assert.ok(report.checks.includes("DST administrator removal confirmation does not promise live revocation"));
  assert.ok(report.checks.includes("DST administrator removal explanation fits the controls column"));
  assert.ok(report.checks.includes("DST removes only the saved Klei administrator entry and retains unverified live admin status"));
  assert.ok(report.checks.includes("DST administrator removal preserves unrelated saved administrators"));
  assert.ok(report.checks.includes("DST ordinary players retain their declared kick action"));
  assert.ok(report.checks.includes("DST ordinary kick feedback confirms sending without claiming the player was removed"));
  assert.ok(report.checks.includes("Persistent player action feedback does not widen or clip the controls and buttons"));
  assert.ok(report.checks.includes("Persistent player action feedback remains fully readable inside the controls"));
  assert.ok(report.checks.includes("DST unlisted administrator explanation is reachable by keyboard"));
  assert.ok(report.checks.includes("DST administrator removal preview keeps its restart warning readable without a help overlay"));
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`PLAYER_DETAILS_BROWSER ${JSON.stringify(report)}`);
});
