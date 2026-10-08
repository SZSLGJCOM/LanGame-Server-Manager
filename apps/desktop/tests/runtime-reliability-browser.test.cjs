const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const { runBrowserFixture, removeScratch } = require("./helpers/runtime-browser-process.cjs");

test("viewport preparation does not depend on a preliminary HTTP document", { timeout: 90000 }, async () => {
  let preparationRequests = 0;
  const viewport = { width: 960, height: 600 };
  const report = await runBrowserFixture({ viewport, deviceScaleFactor: 1.5, reducedMotion: "reduce", browserStartupTimeoutMs: 20000,
    fixtureMiddleware(request, response, next) {
      if (request.url.startsWith("/__reliability_viewport/")) {
        preparationRequests += 1;
        response.writeHead(503).end();
      } else next();
    },
  });
  assert.equal(preparationRequests, 0);
  assert.equal(report.status, "passed");
  assert.deepEqual(report.viewport, viewport);
  assert.equal(report.device_scale_factor, 1.5);
  assert.equal(report.reduced_motion, true);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  assert.equal(report.browser_startup.timeout_ms, 20000);
});

test("reliability real ReactDOM StrictMode lifecycle releases subscriptions and bounds the DOM tail", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture();
  assert.equal(report.status, "passed");
  assert.equal(report.cycles, 12);
  assert.equal(report.renders, 36);
  assert.equal(report.strict_mode_replays, report.cycles);
  assert.ok(report.listeners_created >= report.cycles * 4);
  assert.equal(report.listeners_released, report.listeners_created);
  assert.equal(report.listeners_remaining, 0);
  assert.equal(report.dom_nodes_after_unmount, 0);
  assert.equal(report.max_tail_lines, 400);
  assert.equal(report.readbacks, report.cycles);
  assert.deepEqual(report.browser_errors, []);
  assert.deepEqual(report.checks, [
    "strict_mode_replay", "same_root_path_change", "same_root_instance_change",
    "late_registration", "late_events", "bounded_dom_tail", "unmount_baseline", "service_generation_readback",
  ]);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_startup.timeout_ms, 15000);
  assert.ok(Number.isFinite(report.browser_startup.endpoint_ready_ms));
  assert.equal(report.browser_startup.viewport_ready_ms, null);
  assert.equal(report.browser_startup.browser_product, report.browser_product);
  assert.ok(Number.isFinite(report.browser_startup.exit.elapsed_ms));
  assert.ok(report.browser_startup.host_resources.available_parallelism > 0);
  assert.ok(report.browser_startup.host_resources.total_memory_bytes > 0);
  assert.ok(report.browser_startup.host_resources.sampled_for_ms >= report.browser_startup.endpoint_ready_ms);
  assert.ok(report.browser_processes_observed >= 2);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`RELIABILITY_BROWSER ${JSON.stringify(report)}`);
});

test("reliability browser startup failure preserves its cause and unverified process-tree scratch", { timeout: 30000 }, async () => {
  const configuredBrowser = process.env.LANGAME_RELIABILITY_BROWSER;
  let failure;
  try {
    // Node rejects Chrome's command-line switches before running any script or
    // spawning children. Its real process exit cannot provide a CDP snapshot.
    process.env.LANGAME_RELIABILITY_BROWSER = process.execPath;
    try { await runBrowserFixture(); } catch (error) { failure = error; }
  } finally {
    if (configuredBrowser === undefined) delete process.env.LANGAME_RELIABILITY_BROWSER;
    else process.env.LANGAME_RELIABILITY_BROWSER = configuredBrowser;
  }
  try {
    assert.ok(failure instanceof AggregateError);
    assert.match(failure.errors[0].message, /Browser exited during startup/);
    assert.match(failure.message, /process-tree exit is unverified/);
    assert.equal(failure.browserExited, true);
    assert.equal(failure.browserStartup.endpoint_ready_ms, null);
    assert.equal(failure.browserStartup.viewport_ready_ms, null);
    assert.equal(failure.browserStartup.browser_product, null);
    assert.ok(Number.isFinite(failure.browserStartup.failed_after_ms));
    assert.ok(Number.isFinite(failure.browserStartup.exit.elapsed_ms));
    assert.ok(failure.browserStartup.host_resources.available_parallelism > 0);
    assert.ok(failure.browserStartup.host_resources.total_memory_bytes > 0);
    assert.ok(Number.isSafeInteger(failure.browserPid));
    assert.ok(failure.retainedScratch, "A missing snapshot must retain the fixture directory");
    assert.equal((await fs.stat(failure.retainedScratch)).isDirectory(), true);
  } finally {
    // Only this deliberately invalid Node launch is known to have no children.
    // The production cleanup path correctly leaves an unverified browser alone.
    if (failure?.retainedScratch) {
      assert.throws(() => process.kill(failure.browserPid, 0), { code: "ESRCH" });
      await removeScratch(failure.retainedScratch);
    }
  }
});
