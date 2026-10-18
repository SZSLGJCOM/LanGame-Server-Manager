const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
test("Mod drop entry accepts file, link and selected text payloads across declared game providers", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "mod-drop-browser.html", viewport: { width: 1560, height: 900 } });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.covered.length, 15);
  assert.deepEqual(report.workshop_covered, ["barotrauma", "conanexiles", "palworld", "squad", "terraria"]);
  assert.deepEqual(report.version_policy_covered, ["corekeeper", "valheim", "vrising"]);
  assert.equal(report.stages, 30, "Four Nexus providers additionally install an explicitly selected reviewed file");
  assert.ok(report.references >= 6);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(JSON.stringify({ covered: report.covered, workshop_covered: report.workshop_covered,
    checks: report.checks.length, references: report.references, files: report.stages }));
});
