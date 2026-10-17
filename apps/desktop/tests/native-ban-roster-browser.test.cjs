const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("nine native ban rosters share header navigation and exact offline unban", { timeout: 120000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "native-ban-roster-browser.html", fixtureCleanup: true,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_NATIVE_BANS_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  for (const locale of ["zh-CN", "en-US"]) for (const game of ["conanexiles", "palworld", "rimworld", "soulmask",
    "arksurvivalascended", "arksurvivalevolved", "squad", "necesse", "unturned"]) {
    for (const description of ["ban list is adjacent to online players in header tablist", "loading does not fabricate entries or authority",
      "offline native entry only enables exact unban", "native unban is one shared action including RimWorld and Squad",
      "changing identity cancels confirmation", "confirmation retains complete exact account",
      "native unban envelope and duplicate submission guard are exact", "unban refreshes native and online rosters",
      "failed unban invalidates authority while retaining selection", "failed read preserves entries without fresh count",
      "explicit refresh restores native authority", "readonly workspace can inspect and refresh but cannot unban",
      "mismatched module response cannot authorize a roster",
      "confirmed empty native list is explicit"]) {
      assert.ok(report.checks.includes(`${locale}/${game}: ${description}`), `${locale}/${game}: missing ${description}`);
    }
  }
  assert.ok(report.checks.includes("Late previous-instance read cannot replace current native roster"));
  for (const locale of ["zh-CN", "en-US"]) assert.ok(report.checks.includes(
    `${locale}/palworld: historical player GUID remains visible without inventing a REST user identity or unban`));
  for (const locale of ["zh-CN", "en-US"]) {
    for (const description of ["stopped Squad can remove a persisted ban", "Squad confirmation requires restart to take effect",
      "Squad persistent removal reports pending restart without claiming immediate unban", "Squad has no ineffective reload recovery action",
      "stopped Squad removal refreshes persistence without requesting online players",
      "Squad reban during removal remains unconfirmed when the saved entry reappears",
      "Squad failed post-removal read cannot confirm persisted removal"]) {
      assert.ok(report.checks.includes(`${locale}/squad: ${description}`));
    }
    for (const game of ["conanexiles", "palworld", "rimworld", "soulmask", "arksurvivalascended", "arksurvivalevolved", "necesse", "unturned"]) {
      assert.ok(report.checks.includes(`${locale}/${game}: stopped server retains bans without native mutation`));
      assert.ok(report.checks.includes(`${locale}/${game}: submitted command is not success while native entry remains`));
    }
    for (const description of ["Epic ban remains visible without matching a Steam player or name", "Epic ban confirmation preserves both complete identity components",
      "Epic persisted unban submits the exact pair without normalization", "Epic manual unban confirms the full looked-up identity",
      "Epic manual unban retains the exact declared action envelope", "Epic manual ban confirms the complete exact identity",
      "Epic manual ban retains the exact declared action envelope", "incomplete Epic manual identity cannot authorize ban or unban"]) {
      assert.ok(report.checks.includes(`${locale}/soulmask: ${description}`));
    }
  }
  for (const locale of ["zh-CN", "en-US"]) {
    assert.ok(report.checks.includes(`${locale}/squad: Squad cannot fall back to an undeclared manual unban`));
    assert.ok(report.checks.includes(`${locale}/palworld: manual player GUID cannot fall back to REST user unban`));
    for (const game of ["conanexiles", "palworld", "rimworld", "soulmask", "arksurvivalascended", "arksurvivalevolved", "necesse", "unturned"]) {
      for (const description of ["initial native read failure preserves one explicit manual unban", "manual unban confirms the exact looked-up target",
        "manual unban retains declared IPC without treating unavailable persistence as confirmation", "empty native list cannot confirm a manual unban"]) {
        assert.ok(report.checks.includes(`${locale}/${game}: ${description}`));
      }
    }
  }
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.fixture_cleanup.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`NATIVE_BAN_ROSTER_BROWSER ${JSON.stringify({ status: report.status, checks: report.checks.length,
    browser_product: report.browser_product, browser_processes_remaining: report.browser_processes_remaining, scratch_removed: report.scratch_removed })}`);
});
