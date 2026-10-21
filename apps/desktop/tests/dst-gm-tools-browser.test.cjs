const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const itemsByHeight = new Map();

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }, { width: 1366, height: 768 }, { width: 1920, height: 1080 }]) {
  test(`DST GM tools act on the map's selected player at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "dst-gm-tools-browser.html", viewport,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `dst-gm-spawn-items-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.checks >= 25, "Panel, dispatch, targeting and layout checks must complete");
    assert.equal(report.commands, 15, "Two phases, season, rain, two revives, restore, god mode, unlock, two teleports, clear and three gives (one a typed code) must each dispatch once; the stopped shard none");
    assert.ok(report.items >= 1, "The item page holds at least one row");
    itemsByHeight.set(viewport.height, report.items);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DST_GM_TOOLS_BROWSER ${JSON.stringify(report)}`);
  });
}

test("Taller viewports show more items per page", () => {
  const counts = [...itemsByHeight].sort(([a], [b]) => a - b).map(([, items]) => items);
  // Skipped when a name pattern ran only some viewports.
  if (counts.length < 2) return;
  assert.ok(counts.every((items, index) => index === 0 || items >= counts[index - 1]) && counts.at(-1) > counts[0], `Item counts by height: ${JSON.stringify([...itemsByHeight])}`);
});
