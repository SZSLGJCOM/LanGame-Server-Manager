const assert = require("node:assert/strict");
const test = require("node:test");
const childProcess = require("node:child_process");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { hostResourceUsage, runBrowserFixture, browserTimeBudgets, waitForBrowserEndpoint,
  fixtureCacheDirectory, removeFixtureCache, browserCommand, browserLogEnvironment,
  readBrowserLog } = require("./helpers/runtime-browser-process.cjs");

test("browser diagnostics stay in the child-owned directory and retain only the log tail", async (t) => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-diagnostics-"));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const scratch = path.join(root, "owned");
  await fs.mkdir(scratch);
  const inheritedLog = path.join(root, "other-browser.log");
  await fs.writeFile(inheritedLog, "unrelated");
  const previous = process.env.CHROME_LOG_FILE;
  process.env.CHROME_LOG_FILE = inheritedLog;
  t.after(() => {
    if (previous === undefined) delete process.env.CHROME_LOG_FILE;
    else process.env.CHROME_LOG_FILE = previous;
  });
  const environment = browserLogEnvironment(scratch);
  assert.equal(environment.CHROME_LOG_FILE, path.join(scratch, "chrome_debug.log"));
  assert.equal(process.env.CHROME_LOG_FILE, inheritedLog, "Logging must not mutate the parent environment");
  const tail = "x".repeat(64 * 1024 - 5) + "last\n";
  await fs.writeFile(environment.CHROME_LOG_FILE, "discarded-prefix\n" + tail);
  assert.deepEqual(await readBrowserLog(scratch), { state: "read", truncated: true, tail });
  assert.equal(await fs.readFile(inheritedLog, "utf8"), "unrelated");
});

test("missing or unreadable browser diagnostics remain observations, not replacement failures", async (t) => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-diagnostics-"));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  assert.deepEqual(await readBrowserLog(root), { state: "missing", code: "ENOENT" });
  t.mock.method(fs, "open", async (file) => {
    assert.equal(file, path.join(root, "chrome_debug.log"));
    throw Object.assign(new Error("Unavailable log"), { code: "EACCES" });
  });
  assert.deepEqual(await readBrowserLog(root), { state: "read-error", code: "EACCES" });
});

test("CDP timeout diagnostics distinguish an unopened socket from an unanswered command", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let clock = 0;
  t.mock.method(performance, "now", () => clock);
  for (const opened of [false, true]) {
    let socket;
    let closed = false;
    let sends = 0;
    t.mock.method(globalThis, "WebSocket", function () {
      socket = new EventTarget();
      socket.send = () => { sends += 1; };
      socket.close = () => { closed = true; };
      return socket;
    });
    const pending = browserCommand("ws://controlled.invalid", "Target.createTarget", { url: "about:blank" });
    const rejected = assert.rejects(pending, (error) => {
      assert.match(error.message, /^Target\.createTarget exceeded 3000 ms; CDP progress:/);
      assert.deepEqual(JSON.parse(error.message.split("CDP progress: ")[1]),
        { socket_open_ms: opened ? 7 : null, command_sent_ms: opened ? 7 : null });
      return true;
    });
    clock += 7;
    if (opened) socket.dispatchEvent(new Event("open"));
    t.mock.timers.tick(2999);
    await nextTurn();
    assert.equal(closed, false, "Diagnostics must not change the three-second limit");
    t.mock.timers.tick(1);
    await rejected;
    assert.equal(closed, true);
    assert.equal(sends, Number(opened));
  }
});

test("successful CDP commands keep their result and release their socket", async (t) => {
  let socket;
  let closed = false;
  const result = { targetId: "owned-target" };
  t.mock.method(globalThis, "WebSocket", function () {
    socket = new EventTarget();
    socket.send = (text) => {
      assert.deepEqual(JSON.parse(text), { id: 1, method: "Target.createTarget", params: { url: "about:blank" } });
      socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ id: 1, result }) }));
    };
    socket.close = () => { closed = true; };
    return socket;
  });
  const pending = browserCommand("ws://controlled.invalid", "Target.createTarget", { url: "about:blank" });
  socket.dispatchEvent(new Event("open"));
  assert.deepEqual(await pending, result);
  assert.equal(closed, true);
});

test("the worker reuses only the same fixture's compiled dependency cache", async (t) => {
  t.after(removeFixtureCache);
  const options = { fixturePath: "runtime-browser.html" };
  const directory = await fixtureCacheDirectory(options);
  await fs.mkdir(directory);
  await fs.writeFile(path.join(directory, "compiled-marker"), "retained");
  assert.equal(await fixtureCacheDirectory(options), directory);
  assert.equal(await fs.readFile(path.join(await fixtureCacheDirectory(options), "compiled-marker"), "utf8"), "retained");
  const others = await Promise.all([
    fixtureCacheDirectory({ fixturePath: "hover-help-browser.html" }),
    fixtureCacheDirectory({ ...options, development: false }),
    fixtureCacheDirectory({ ...options, reactTransform: false }),
  ]);
  assert.equal(new Set([directory, ...others]).size, 4, "Different fixtures and compilation modes cannot share an optimizer cache");
  assert.equal(path.dirname(path.dirname(directory)), path.resolve(os.tmpdir()));
});

test("a cache cleanup failure remains visible and retains its owned directory for cleanup", async (t) => {
  t.after(async () => { t.mock.restoreAll(); await removeFixtureCache(); });
  const options = { fixturePath: "runtime-browser.html" };
  const directory = await fixtureCacheDirectory(options);
  const root = path.dirname(directory);
  const failure = Object.assign(new Error("Owned cache could not be removed"), { code: "EACCES" });
  const remove = fs.rm;
  t.mock.method(fs, "rm", async (target, ...args) => {
    if (target === root) throw failure;
    return remove(target, ...args);
  });
  await assert.rejects(removeFixtureCache(), (error) => error === failure);
  assert.equal(await fixtureCacheDirectory(options), directory, "Failed cleanup must not abandon ownership");
  assert.equal((await fs.stat(root)).isDirectory(), true);
});

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
  const closing = closeBrowser(child, exited, undefined, performance.now() + 5000);
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
  assert.equal(await confirmBrowserExit(child, [12345, 12346], true, new Error("Concurrent exit"), performance.now() + 3000), true);
  assert.deepEqual(checked, [12345, 12346]);
});

test("a missing snapshot cannot prove tree exit even after its root exited", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  t.mock.method(process, "kill", () => assert.fail("No snapshot is available to inspect"));
  const child = { pid: 12345, exitCode: 0, signalCode: null };
  assert.equal(await confirmBrowserExit(child, [], false, null, performance.now() + 3000), false);
  const terminationError = new Error("Termination could not verify its tree");
  await assert.rejects(confirmBrowserExit(child, [], false, terminationError, performance.now() + 3000), (error) => error === terminationError);
});

test("a live descendant fails verification and preserves the original termination error", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(signal, 0);
    if (pid === 12345) throw Object.assign(new Error("Root exited"), { code: "ESRCH" });
    assert.equal(pid, 12346);
  });
  t.mock.method(performance, "now", () => 3001);
  const terminationError = new Error("The descendant could not be terminated");
  await assert.rejects(confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12345, 12346], true, terminationError, 3000), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors[0], terminationError);
    assert.match(error.errors[1].message, /Owned browser cleanup exhausted its shared command time budget/);
    return true;
  });
});

test("an incomplete snapshot cannot certify the owned browser exit", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("No command may run during exit verification"));
  await assert.rejects(confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12346], true, null, performance.now() + 3000),
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
  t.mock.method(performance, "now", () => 0);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const closing = closeBrowser({ pid: 12345, exitCode: null, signalCode: null }, new Promise(() => {}), undefined, 5000);
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

test("owned root exit beyond five seconds uses the existing command's remaining cleanup budget", async (t) => {
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(pid, -12345);
    assert.equal(signal, "SIGKILL");
  });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => callback(null, "", ""));
  let now = 0;
  t.mock.method(performance, "now", () => now);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const child = { pid: 12345, exitCode: null, signalCode: null };
  let finishExit;
  const exited = new Promise((resolve) => { finishExit = resolve; });
  // The existing default 80-second command leaves six seconds for server and
  // scratch cleanup. No local phase is allowed to extend that absolute ceiling.
  const closing = closeBrowser(child, exited, undefined, 74000);
  let settled = false;
  closing.then(() => { settled = true; }, () => { settled = true; });
  await nextTurn();
  now = 6000;
  t.mock.timers.tick(6000);
  await nextTurn();
  assert.equal(settled, false, "Five seconds must not preempt the shared cleanup budget");
  child.exitCode = 0;
  finishExit({ code: 0, signal: null });
  assert.equal(await closing, null);
});

test("a root that never exits still fails at the remaining absolute cleanup deadline", async (t) => {
  t.mock.method(process, "kill", () => {});
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => callback(null, "", ""));
  // The fixture has already consumed 60 seconds of the same 80-second command.
  let now = 60000;
  t.mock.method(performance, "now", () => now);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const child = { pid: 12345, exitCode: null, signalCode: null };
  const closing = closeBrowser(child, new Promise(() => {}), undefined, 74000);
  let settled = false;
  closing.then(() => { settled = true; }, () => { settled = true; });
  const rejected = assert.rejects(closing, /Owned browser process exit exceeded 14000 ms/);
  await nextTurn();
  now = 73999;
  t.mock.timers.tick(13999);
  await nextTurn();
  assert.equal(settled, false);
  now = 74000;
  t.mock.timers.tick(1);
  await rejected;
  assert.equal(child.exitCode, null, "A timeout must not invent an exit code");
});

test("descendants consume the root's same absolute cleanup budget without restarting a phase timer", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("Exit confirmation cannot terminate a process"));
  let now = 73990;
  t.mock.method(performance, "now", () => now);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(signal, 0);
    if (pid === 12345) throw Object.assign(new Error("Root exited"), { code: "ESRCH" });
    assert.equal(pid, 12346);
  });
  const pending = confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12345, 12346], true, null, 74000);
  const rejected = assert.rejects(pending, /Owned browser cleanup exhausted its shared command time budget/);
  now = 74000;
  t.mock.timers.tick(10);
  await rejected;
});

test("a descendant that exits within the remaining budget can outlive its root by more than three seconds", async (t) => {
  const { confirmBrowserExit } = loadLifecycle(t, () => assert.fail("Exit confirmation cannot terminate a process"));
  let now = 60000;
  t.mock.method(performance, "now", () => now);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let descendantAlive = true;
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(signal, 0);
    if (pid === 12345 || !descendantAlive) throw Object.assign(new Error("Exited"), { code: "ESRCH" });
    assert.equal(pid, 12346);
  });
  const pending = confirmBrowserExit({ pid: 12345, exitCode: 0, signalCode: null }, [12345, 12346], true, null, 74000);
  now = 64000;
  descendantAlive = false;
  t.mock.timers.tick(4000);
  assert.equal(await pending, true);
});

test("an exhausted observation budget still requests owned termination and cannot invent root exit", async (t) => {
  let terminationRequests = 0;
  t.mock.method(process, "kill", (pid, signal) => {
    assert.equal(pid, -12345);
    assert.equal(signal, "SIGKILL");
    terminationRequests += 1;
  });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => {
    assert.equal(file, "taskkill.exe");
    assert.deepEqual(args, ["/PID", "12345", "/T", "/F"]);
    assert.equal(options.timeout, 5000);
    terminationRequests += 1;
    callback(null, "", "");
  });
  t.mock.method(performance, "now", () => 75000);
  const child = { pid: 12345, exitCode: null, signalCode: null };
  await assert.rejects(closeBrowser(child, new Promise(() => {}), undefined, 74000),
    /Owned browser cleanup exhausted its shared command time budget/);
  assert.equal(terminationRequests, 1, "An elapsed wait budget must not skip the owned termination request");
  assert.equal(child.exitCode, null);
});

test("graceful close exhausting the observation budget still falls back to one owned termination request", async (t) => {
  let now = 73999;
  let terminationRequests = 0;
  let socketClosed = false;
  t.mock.method(performance, "now", () => now);
  t.mock.method(globalThis, "WebSocket", function () {
    return {
      addEventListener(event, listener) { if (event === "open") queueMicrotask(listener); },
      send(message) {
        assert.equal(JSON.parse(message).method, "Browser.close");
        now = 74000;
      },
      close() { socketClosed = true; },
    };
  });
  t.mock.method(process, "kill", () => { terminationRequests += 1; });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => {
    assert.equal(options.timeout, 5000);
    terminationRequests += 1;
    callback(null, "", "");
  });
  const child = { pid: 12345, exitCode: null, signalCode: null };
  await assert.rejects(closeBrowser(child, new Promise(() => {}), "ws://controlled.invalid", 74000),
    /Owned browser cleanup exhausted its shared command time budget/);
  assert.equal(socketClosed, true);
  assert.equal(terminationRequests, 1);
  assert.equal(child.exitCode, null);
});

test("an expired close budget skips the DevTools connection before requesting owned termination", async (t) => {
  t.mock.method(performance, "now", () => 75000);
  let connectionAttempts = 0;
  t.mock.method(globalThis, "WebSocket", function () {
    connectionAttempts += 1;
    throw new Error("No DevTools connection is permitted after the observation budget expires");
  });
  let terminationRequests = 0;
  t.mock.method(process, "kill", () => { terminationRequests += 1; });
  const { closeBrowser } = loadLifecycle(t, (file, args, options, callback) => {
    assert.equal(file, "taskkill.exe");
    assert.deepEqual(args, ["/PID", "12345", "/T", "/F"]);
    terminationRequests += 1;
    callback(null, "", "");
  });
  const child = { pid: 12345, exitCode: null, signalCode: null };
  await assert.rejects(closeBrowser(child, new Promise(() => {}), "ws://controlled.invalid", 74000),
    /Owned browser cleanup exhausted its shared command time budget/);
  await nextTurn();
  assert.equal(connectionAttempts, 0, "An expired observation budget must not create a socket or an unobserved connection promise");
  assert.equal(terminationRequests, 1);
  assert.equal(child.exitCode, null);
});
