const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const fs = require("node:fs/promises");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`ASTRONEER persisted connection flow at ${viewport.width}x${viewport.height}`, { timeout: 120000 }, async () => {
    const artifactDirectory = process.env.LANGAME_ASTRO_CONNECTION_ARTIFACT_DIR;
    const artifactName = `astroneer-connection-${viewport.width}x${viewport.height}`;
    const report = await runBrowserFixture({ fixturePath: "astroneer-connection-browser.html", viewport,
      screenshotPath: artifactDirectory ? path.join(artifactDirectory, `${artifactName}.png`) : undefined,
      keyboard: true, fixtureCleanup: true, fixtureTimeoutMs: 60000 });
    if (artifactDirectory) await fs.writeFile(path.join(artifactDirectory, `${artifactName}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.deepEqual(report.fixture_cleanup.browser_errors, []);
    assert.equal(report.fixture_cleanup.native_dialogs, 0);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.browser_exited, true);
    assert.equal(report.scratch_removed, true);
    console.log(`ASTRONEER_CONNECTION_BROWSER ${JSON.stringify({ viewport, checks: report.checks.length, writes: report.writes })}`);
  });
}
