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

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`configuration status notices retain recovery and cross-category navigation at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: "configuration-catalog-browser.html", viewport,
      fixtureCleanup: true, bundleWithMiddleware: true, fixtureMiddleware: fixtureMode("configuration-status-notices") });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.notice_actions, 3);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}

for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`DST Cursed Confrontation Caves settings save all five choices at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true, bundleWithMiddleware: true,
      fixtureMiddleware: fixtureMode("dontstarve-cursed-confrontation"),
      screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
        ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, `dontstarve-caves-bosses-${viewport.width}x${viewport.height}.png`)
        : undefined
    });
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.caves_boss_titles, ["猎犬蝙蝠", "岩石大师"]);
    assert.deepEqual(report.boss_value_readbacks, {
      caves_bat_boss: ["never", "rare", "default", "often", "always"],
      caves_rocky_boss: ["never", "rare", "default", "often", "always"]
    });
    assert.equal(report.master_boss_controls, 0);
    assert.equal(report.setting_save_readbacks, 10);
    assert.equal(report.save_boundary, "synthetic onSave; no server files written");
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`DST_CURSED_CONFRONTATION_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
  });
}

test("Core Keeper preserves existing 32-character passwords through input, save and reload", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({
    fixturePath: "configuration-catalog-browser.html", viewport: { width: 1560, height: 900 }, fixtureCleanup: true, bundleWithMiddleware: true,
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

test("Enshrouded 1.0 Flame levels and Automatic enemy amount survive configuration saves", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture({
    fixturePath: "configuration-catalog-browser.html", viewport: { width: 1560, height: 900 }, fixtureCleanup: true, bundleWithMiddleware: true,
    fixtureMiddleware: fixtureMode("enshrouded-1-0"),
    screenshotPath: process.env.LANGAME_CONFIGURATION_REVIEW_DIR
      ? path.join(process.env.LANGAME_CONFIGURATION_REVIEW_DIR, "enshrouded-1-0-settings-1560x900.png")
      : undefined
  });
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.default_flame_level, 11);
  assert.deepEqual(report.mock_flame_level_readbacks, [1, 11]);
  assert.equal(report.mock_enemy_amount_readback, "Automatic");
  assert.equal(report.setting_save_readbacks, 3);
  assert.equal(report.save_boundary, "synthetic onSave; no server files written");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`ENSHROUDED_1_0_CONFIGURATION_BROWSER ${JSON.stringify(report)}`);
});

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`all game configuration categories render at ${viewport.width}×${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true, bundleWithMiddleware: true,
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
      fixturePath: "configuration-catalog-browser.html", viewport: { width: 1560, height: 900 }, fixtureCleanup: true, bundleWithMiddleware: true,
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
      fixturePath: "configuration-catalog-browser.html", viewport, fixtureCleanup: true, bundleWithMiddleware: true,
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
