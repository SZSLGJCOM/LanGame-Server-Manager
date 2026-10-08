const assert = require("node:assert/strict");
const test = require("node:test");
const childProcess = require("node:child_process");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { hostResourceUsage, runBrowserFixture, browserTimeBudgets, waitForBrowserEndpoint } = require("./helpers/runtime-browser-process.cjs");

test("startup phase limits do not extend the shared command watchdog or fixture budget", () => {
  const defaults = browserTimeBudgets();
  assert.deepEqual(defaults, { fixtureTimeoutMs: 45000, browserStartupTimeoutMs: 15000, commandTimeoutMs: 80000 });
  for (const browserStartupTimeoutMs of [1000, 15000, 20000, 30000]) {
    const budgets = browserTimeBudgets({ browserStartupTimeoutMs });
    assert.equal(budgets.commandTimeoutMs, defaults.commandTimeoutMs);
    assert.equal(budgets.fixtureTimeoutMs, defaults.fixtureTimeoutMs);
    assert.equal(budgets.browserStartupTimeoutMs, browserStartupTimeoutMs);
  }
  assert.equal(browserTimeBudgets({ fixtureTimeoutMs: 1000 }).commandTimeoutMs, 36000);
});

test("cold browser endpoint readiness can exceed five seconds within its separate startup budget", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ready;
  const pending = waitForBrowserEndpoint(new Promise((resolve) => { ready = resolve; }), new Promise(() => {}));
  const completed = assert.doesNotReject(pending);
  t.mock.timers.tick(5219); // Observed hosted Windows cold-start sample.
  ready("ws://controlled.invalid/devtools");
  await completed;
  assert.equal(await pending, "ws://controlled.invalid/devtools");
});

test("a missing browser endpoint expires at its configured startup budget", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let settled = false;
  const pending = waitForBrowserEndpoint(new Promise(() => {}), new Promise(() => {}), 1200);
  pending.then(() => { settled = true; }, () => { settled = true; });
  const rejected = assert.rejects(pending, /^Error: Browser DevTools startup exceeded 1200 ms$/);
  t.mock.timers.tick(1199);
  await nextTurn();
  assert.equal(settled, false);
  t.mock.timers.tick(1);
  await rejected;
});

test("browser exit during startup is reported before the endpoint deadline", async () => {
  await assert.rejects(waitForBrowserEndpoint(new Promise(() => {}), Promise.resolve({ code: 2 })),
    /Browser exited during startup: \{"code":2\}/);
});

test("invalid startup budgets are rejected before browser launch", async () => {
  for (const browserStartupTimeoutMs of [0, -1, 30001, 1.5, NaN, Infinity]) {
    await assert.rejects(runBrowserFixture({ browserStartupTimeoutMs }), /Browser startup timeout must be/);
  }
});

test("browser startup reports interval CPU use and host memory without lifetime averages", () => {
  const start = { sampled_at_ms: 100, available_parallelism: 2, logical_cpus: 4,
    total_memory_bytes: 16 * 1024 ** 3, free_memory_bytes: 8 * 1024 ** 3, cpu_total_ms: 100000, cpu_idle_ms: 10000 };
  const end = { ...start, sampled_at_ms: 600, free_memory_bytes: 7 * 1024 ** 3,
    cpu_total_ms: 102000, cpu_idle_ms: 10500 };
  assert.deepEqual(hostResourceUsage(start, end), {
    available_parallelism: 2, logical_cpus: 4, total_memory_bytes: 16 * 1024 ** 3,
    free_memory_start_bytes: 8 * 1024 ** 3, free_memory_end_bytes: 7 * 1024 ** 3,
    sampled_for_ms: 500, system_cpu_busy_fraction: 0.75,
  });
});

test("browser startup does not turn unavailable or reset CPU counters into idle evidence", () => {
  const start = { sampled_at_ms: 100, available_parallelism: 4, logical_cpus: 4,
    total_memory_bytes: 16 * 1024 ** 3, free_memory_bytes: 8 * 1024 ** 3, cpu_total_ms: 100000, cpu_idle_ms: 10000 };
  for (const end of [start, { ...start, logical_cpus: 0 },
    { ...start, logical_cpus: 8, cpu_total_ms: 200000 },
    { ...start, cpu_total_ms: 1000, cpu_idle_ms: 100 },
    { ...start, cpu_total_ms: 100010, cpu_idle_ms: 10100 }]) {
    assert.equal(hostResourceUsage(start, end).system_cpu_busy_fraction, null);
  }
});

function loadLifecycle(t, execFile) {
  const helper = require.resolve("./helpers/runtime-browser-process.cjs");
  const originalExecFile = childProcess.execFile;
  // A mock proxy inherits execFile's custom promisifier and would call the OS.
  // Replace this boundary with a plain callback function before loading it.
  childProcess.execFile = execFile;
  delete require.cache[helper];
  t.after(() => {
    childProcess.execFile = originalExecFile;
    delete require.cache[helper];
  });
  return require(helper);
}

test("browser cleanup waits for the owned root after taskkill reports an exited descendant", async (t) => {
  const terminationError = new Error("A descendant exited while taskkill was enumerating the tree");
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(pid, -12345);
    assert.equal(signal, "SIGKILL");
    throw terminationError;
  });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => {
    assert.equal(file, "taskkill.exe");
    assert.deepEqual(args, ["/PID", "12345", "/T", "/F"]);
    callback(terminationError);
  });
  const child = { pid: 12345, exitCode: null, signalCode: null };
  let finishExit;
  const exited = new Promise((resolve) => { finishExit = resolve; });
  let settled = false;
  const closing = closeBrowser(child, exited, undefined);
  closing.then(() => { settled = true; }, () => { settled = true; });
  await nextTurn();
  const settledBeforeExit = settled;
  child.exitCode = 0;
  finishExit({ code: 0, signal: null });
  const diagnostic = await closing.catch((error) => error);
  assert.equal(settledBeforeExit, false, "A termination error must not skip owned-root exit observation");
  assert.equal(diagnostic, terminationError, "Keep the termination error until the full tree is verified");
});

test("a taskkill error is nonfatal only after every snapshot PID and the root have exited", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  const checked = [];
  t.mock.method(process, "kill", (pid, signal) => {
    checked.push(pid);
    assert.equal(signal, 0);
    throw Object.assign(new Error("Exited"), { code: "ESRCH" });
  });
  const child = { pid: 12345, exitCode: 0, signalCode: null };
  assert.equal(await confirmBrowserExit(child, [12345, 12346], true, new Error("Concurrent exit")), true);
  assert.deepEqual(checked, [12345, 12346]);
});

test("a missing snapshot cannot prove tree exit even after its root exited", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  t.mock.method(process, "kill", () => assert.fail("No snapshot is available to inspect"));
  const child = { pid: 12345, exitCode: 0, signalCode: null };
  assert.equal(await confirmBrowserExit(child, [], false, null), false);
  const terminationError = new Error("Termination could not verify its tree");
  await assert.rejects(confirmBrowserExit(child, [], false, terminationError), (error) => error === terminationError);
});

test("a live descendant fails verification and preserves the original termination error", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(signal, 0);
    if (pid === 12345) throw Object.assign(new Error("Root exited"), { code: "ESRCH" });
    assert.equal(pid, 12346);
  });
  let now = 0;
  t.mock.method(Date, "now", () => { now += 3001; return now; });
  const terminationError = new Error("The descendant could not be terminated");
  await assert.rejects(confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12345, 12346], true, terminationError), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors[0], terminationError);
    assert.match(error.errors[1].message, /Owned browser descendants did not exit/);
    return true;
  });
});

test("an incomplete snapshot cannot certify the owned browser exit", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  await assert.rejects(confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12346], true, null),
    /snapshot must include the owned browser root/);
});

test("an unobserved root exit remains bounded and preserves the termination error", async (t) => {
  const terminationError = new Error("Root termination failed");
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(pid, -12345);
    assert.equal(signal, "SIGKILL");
    throw terminationError;
  });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => callback(terminationError));
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const closing = closeBrowser({ pid: 12345, exitCode: null, signalCode: null }, new Promise(() => {}), undefined);
  const rejected = assert.rejects(closing, (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors[0], terminationError);
    assert.match(error.errors[1].message, /Owned browser process exit exceeded 5000 ms/);
    return true;
  });
  await nextTurn();
  t.mock.timers.tick(5000);
  await rejected;
});
