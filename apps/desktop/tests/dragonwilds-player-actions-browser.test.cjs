const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Dragonwilds real player workspace connects verified native actions and manual EOS unban", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "dragonwilds-player-actions-browser.html", viewport: { width: 1280, height: 900 },
    fixtureCleanup: true, screenshotPath: process.env.LANGAME_DRAGONWILDS_PLAYERS_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  for (const locale of ["zh-CN", "en-US"]) for (const description of [
    "actual player workspace reads through the live-player API hook",
    "kick and ban require current player selection",
    "verified native EOS and session enable kick and ban",
    "current manifest supplies localized action labels",
    "opening kick confirmation sends no command",
    "confirmed kick uses the production action API",
    "one pending native action disables duplicate operations",
    "live action sends only authorized snapshot references",
    "confirmed effect refreshes the real player polling hook",
    "online ban uses native runtime action instead of a settings mutation",
    "native failure is visible without success feedback",
    "query-only fallback never gains account operations",
    "unavailable identity exposes accessible disabled reasons",
    "stale snapshot disables live moderation",
    "stopped server cannot accept manual native operations",
    "manual route contains only unban and cannot bypass current row authorization",
    "Steam64 and session-like targets cannot submit an EOS unban",
    "exact known EOS account enables manual unban",
    "manual unban requires explicit confirmation",
    "manual unban dispatches only the exact EOS account and declared operation",
    "confirmed unban reports its result in accessible feedback",
  ]) assert.ok(report.checks.includes(`${locale}: ${description}`), `Missing ${locale}: ${description}`);
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`DRAGONWILDS_PLAYERS_BROWSER ${JSON.stringify({ status: report.status, checks: report.checks.length })}`);
});
