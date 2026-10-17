const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Dragonwilds banned tab reads persistence and unbans exact offline EOS accounts", { timeout: 120000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "dragonwilds-ban-roster-browser.html",
    viewport: { width: 1280, height: 900 }, fixtureCleanup: true,
    screenshotPath: process.env.LANGAME_DRAGONWILDS_BANS_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  for (const locale of ["zh-CN", "en-US"]) for (const description of [
    "initial loading has no fabricated count or empty state",
    "same-name accounts and unnamed accounts retain complete EOS identities",
    "ban tab retains the fixed action inventory without settings editors",
    "kick ban and unban have one fixed declared order",
    "no selection preserves every action DOM object and expected permission",
    "online selection preserves every action DOM object and expected permission",
    "online deselection preserves every action DOM object and expected permission",
    "banned list without selection preserves every action DOM object and expected permission",
    "offline banned selection preserves every action DOM object and expected permission",
    "another same-name banned account preserves every action DOM object and expected permission",
    "returning online without selection preserves every action DOM object and expected permission",
    "online reselection preserves every action DOM object and expected permission",
    "stale online snapshot preserves every action DOM object and expected permission",
    "fresh persisted account despite stale online snapshot preserves every action DOM object and expected permission",
    "initial roster loading preserves every action DOM object and expected permission",
    "read-only banned selection preserves every action DOM object and expected permission",
    "stopped banned selection preserves every action DOM object and expected permission",
    "selected banned row shows the full EOS without duplicate details or fabricated duration",
    "switching lists cancels a live confirmation without dispatch",
    "changing banned accounts cancels the earlier confirmation without dispatch",
    "switching lists cancels an unban confirmation without dispatch",
    "tab keyboard navigation uses the shared tablist without extra reads",
    "offline account unban needs no online snapshot",
    "confirmation includes the exact selected EOS and sends nothing early",
    "pending unban blocks duplicate requests and selection changes",
    "confirmed unban refreshes both lists and clears selection",
    "native failure retains the exact entry and requires a fresh read",
    "read failure preserves old entries without a fresh count or enabled unban",
    "explicit refresh coalesces concurrent reads and restores authority",
    "read-only workspace permits inspection and refresh but never unban",
    "stopped server retains persisted bans and explains disabled unban",
    "initial read failure is distinct from a confirmed empty roster",
    "online native ban refreshes the independent persisted roster",
    "disconnected online account unban refreshes the persisted roster and online list",
    "a read started before moderation cannot certify its resulting roster",
    "unknown ban results cannot be made fresh by an earlier read",
  ]) assert.ok(report.checks.includes(`${locale}: ${description}`), `Missing ${locale}: ${description}`);
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`DRAGONWILDS_BAN_ROSTER_BROWSER ${JSON.stringify({ status: report.status, checks: report.checks.length,
    browser_product: report.browser_product, browser_processes_remaining: report.browser_processes_remaining,
    scratch_removed: report.scratch_removed })}`);
});
