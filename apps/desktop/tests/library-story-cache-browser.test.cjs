const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("saved Steam descriptions survive reload, stay isolated and bounded, and tolerate unavailable storage", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({ fixturePath: "library-story-cache-browser.html", viewport: { width: 1280, height: 900 } });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.scenarios, ["reload-and-isolation", "expiry-preservation-and-size", "bounded-retention", "storage-denied"]);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`LIBRARY_STORY_CACHE ${JSON.stringify(report)}`);
});
