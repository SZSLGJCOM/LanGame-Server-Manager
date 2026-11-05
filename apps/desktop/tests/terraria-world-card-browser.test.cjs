const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Terraria world conflicts stay on the owning card and require its explicit confirmation", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "terraria-world-card-browser.html", keyboard: true,
    viewport: { width: 960, height: 720 }, screenshotPath: process.env.LANGAME_TERRARIA_WORLD_CARD_SCREENSHOT });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.inline, true);
  assert.equal(report.reviewed_starts, 2);
  assert.equal(report.native_dialogs, 0);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
});
