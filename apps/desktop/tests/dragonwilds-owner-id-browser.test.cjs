const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Dragonwilds OwnerId keeps pasted identities complete and blocks invalid saves in a real browser", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "dragonwilds-owner-id-browser.html",
    viewport: { width: 1440, height: 1000 }, screenshotPath: process.env.LANGAME_OWNER_ID_SCREENSHOT });
  if (process.env.LANGAME_OWNER_ID_REPORT) {
    await fs.writeFile(process.env.LANGAME_OWNER_ID_REPORT, JSON.stringify(report, null, 2));
  }
  console.log(`DRAGONWILDS_OWNER_ID_BROWSER ${JSON.stringify(report)}`);
  assert.equal(report.status, "passed");
  assert.equal(report.cases.length, 6);
  assert.equal(report.accepted_saves, 3);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});
