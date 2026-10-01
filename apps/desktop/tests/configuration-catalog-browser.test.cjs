const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

function fixtureMode(mode) {
  return (request, response, next) => {
    if (request.url !== "/__configuration_review_mode") return next();
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ mode }));
  };
}

test("Core Keeper preserves existing 32-character passwords through input, save and reload", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({
    fixturePath: "configuration-catalog-browser.html", viewport: { width: 1560, height: 900 }, fixtureCleanup: true,
    fixtureMiddleware: fixtureMode("corekeeper-password"),
    screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
      ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, "corekeeper-password-1560x900.png")
      : undefined
  });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.generated_password_length, 28);
  assert.equal(report.password_length, 32);
  assert.equal(report.mock_save_readback, true);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`all game configuration categories render at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true,
      fixtureMiddleware: fixtureMode("catalog"),
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `configuration-${viewport.width}x${viewport.height}.png`)
        : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.modules.length, 32);
    assert.equal(report.mock_save_readback, true);
    assert.equal(report.native_enum_save_readbacks, 5);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`CONFIGURATION_CATALOG_BROWSER ${JSON.stringify(report)}`);
  });
}

for (const section of ["campaign", "network"]) {
  test(`Barotrauma ${section} renders and fractional respawn values survive saving`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport: { width: 1560, height: 900 }, fixtureCleanup: true,
      fixtureMiddleware: fixtureMode(`barotrauma-${section}`),
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `barotrauma-${section}-1560x900.png`)
        : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.section, section);
    assert.equal(report.mock_respawn_interval, 2.5);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`BAROTRAUMA_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
  });
}

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Conan verified settings render and survive saving at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true,
      fixtureMiddleware: fixtureMode("conan-semantics"),
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `conan-semantics-${viewport.width}x${viewport.height}.png`)
        : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.modules.map((module) => module.id), ["conanexiles"]);
    assert.equal(report.setting_save_readbacks, 10);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`CONAN_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
  });
}
