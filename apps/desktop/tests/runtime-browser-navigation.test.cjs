const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { runBrowserFixture, waitForFixtureReport } = require("./helpers/runtime-browser-process.cjs");
const { parseSource, sourceText, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

function heldDocument(t, release, milliseconds) {
  let requests = 0;
  let receive;
  const arrived = new Promise((resolve) => { receive = resolve; });
  const timers = new Set();
  t.after(() => { for (const timer of timers) clearTimeout(timer); });
  return {
    arrived,
    get requests() { return requests; },
    middleware(request, response, next) {
      if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
      requests += 1;
      receive();
      if (milliseconds === null) return;
      const timer = setTimeout(() => {
        timers.delete(timer);
        if (!response.destroyed) release(request, response, next);
      }, milliseconds);
      timers.add(timer);
      response.once("close", () => { clearTimeout(timer); timers.delete(timer); });
    },
  };
}

function navigationDeadlineAfterArrival(t, arrived) {
  const NativeWebSocket = globalThis.WebSocket;
  const nativeSend = NativeWebSocket.prototype.send;
  const nativeSetTimeout = globalThis.setTimeout;
  const nativeClearTimeout = globalThis.clearTimeout;
  let captureDeadline = false;
  let arm;
  const armed = new Promise((resolve) => { arm = resolve; });
  const timer = { cleared: false };
  t.mock.method(NativeWebSocket.prototype, "send", function(message) {
    nativeSend.call(this, message);
    // openViewportSession constructs deadline synchronously after send.
    // Capture exactly that timer, leaving startup/cleanup timers real.
    if (JSON.parse(message).method === "Page.navigate") captureDeadline = true;
  });
  t.mock.method(globalThis, "setTimeout", (callback, milliseconds, ...args) => {
    if (!captureDeadline) return nativeSetTimeout(callback, milliseconds, ...args);
    captureDeadline = false;
    arm(milliseconds);
    // HTTP delivery has no promise to beat a one-second CI wall clock. Expire
    // the actual command deadline only after this case's unanswered request
    // exists; shared-budget arithmetic is independently tested below.
    void arrived.then(() => { if (!timer.cleared) callback(...args); });
    return timer;
  });
  t.mock.method(globalThis, "clearTimeout", (value) => {
    if (value === timer) timer.cleared = true;
    else nativeClearTimeout(value);
  });
  return { armed, timer };
}

function verifiedFailureCleanup(error) {
  assert.ok(error instanceof AggregateError);
  assert.equal(error.errors.length, 1, "A fixture failure must not introduce cleanup failures");
  assert.equal(error.browserExited, true);
  assert.equal(error.retainedScratch, null, "Verified tree exit must release the owned profile");
  assert.throws(() => process.kill(error.browserPid, 0), { code: "ESRCH" });
}

test("startup failure before fixture navigation does not invoke React cleanup", { timeout: 90000 }, async (t) => {
  const NativeWebSocket = globalThis.WebSocket;
  let connections = 0;
  t.mock.method(globalThis, "WebSocket", class extends NativeWebSocket {
    constructor(...args) {
      if (++connections === 1) throw new Error("Controlled initial CDP connection failure");
      super(...args);
    }
  });
  await assert.rejects(runBrowserFixture({ viewport: { width: 960, height: 600 }, fixtureCleanup: true }), (error) => {
    verifiedFailureCleanup(error);
    assert.equal(error.errors[0].message, "Controlled initial CDP connection failure");
    assert.equal(error.browserStartup.fixture_requested_ms, null);
    return true;
  });
});

test("real fixture navigation can use its existing budget after a slow HTML response", { timeout: 90000 }, async (t) => {
  const document = heldDocument(t, (_request, _response, next) => next(), 3500);
  const report = await runBrowserFixture({ viewport: { width: 960, height: 600 }, fixtureMiddleware: document.middleware });
  assert.equal(document.requests, 1);
  assert.equal(report.status, "passed");
  assert.equal(report.cycles, 12);
  assert.equal(report.renders, 36);
  assert.equal(report.strict_mode_replays, report.cycles);
  assert.equal(report.listeners_released, report.listeners_created);
  assert.equal(report.listeners_remaining, 0);
  assert.equal(report.dom_nodes_after_unmount, 0);
  assert.equal(report.max_tail_lines, 400);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});

// The 1-second fixture shares a 36-second command ceiling with startup/cleanup;
// leave the same 10-second outer margin as the ordinary 80/90-second fixtures.
test("navigation that never receives a document expires within the fixture budget and releases its tree", { timeout: 46000 }, async (t) => {
  const document = heldDocument(t, () => assert.fail("The held document must not be released"), null);
  const deadline = navigationDeadlineAfterArrival(t, document.arrived);
  const fixture = runBrowserFixture({ viewport: { width: 960, height: 600 }, fixtureTimeoutMs: 1000,
    fixtureMiddleware: document.middleware });
  const rejected = assert.rejects(fixture, (error) => {
    verifiedFailureCleanup(error);
    const timeout = error.errors[0].message.match(/^Page\.navigate exceeded (\d+) ms$/);
    assert.ok(timeout, error.errors[0].message);
    assert.ok(Number(timeout[1]) > 0 && Number(timeout[1]) <= 1000, "Navigation must use the fixture's remaining budget");
    return true;
  });
  const budget = await Promise.race([
    deadline.armed,
    rejected.then(() => assert.fail("The fixture failed before its navigation deadline was armed")),
  ]);
  assert.equal(budget, 1000, "The real navigation command retains its original fixture budget");
  assert.equal(document.requests, 0, "Arming the CDP deadline does not prove HTTP delivery");
  await rejected;
  assert.equal(document.requests, 1);
  assert.equal(deadline.timer.cleared, true, "The command releases its controlled deadline after rejection");
});

test("the report cannot obtain a fresh budget after navigation consumes part of it", async (t) => {
  // Control the CDP acknowledgement and clock, not a second wall-clock race
  // between a delayed HTML response and Chrome's navigation acknowledgement.
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let now = 0;
  t.mock.method(performance, "now", () => now);
  let acknowledge;
  const session = { command(method, params, timeout) {
    assert.equal(method, "Page.navigate");
    assert.deepEqual(params, { url: "http://fixture.invalid/controlled" });
    assert.equal(timeout, 10000);
    return new Promise((resolve) => { acknowledge = resolve; });
  } };
  let settled = false;
  const waiting = waitForFixtureReport({ session, url: "http://fixture.invalid/controlled",
    result: new Promise(() => {}), exited: new Promise(() => {}), timeoutMs: 10000 });
  waiting.then(() => { settled = true; }, () => { settled = true; });
  const rejected = assert.rejects(waiting, /^Error: Real ReactDOM reliability fixture exceeded 6500 ms$/);
  now = 3500;
  acknowledge({});
  await nextTurn();
  t.mock.timers.tick(6499);
  await nextTurn();
  assert.equal(settled, false, "The report retains exactly the unused part of the shared budget");
  t.mock.timers.tick(1);
  await rejected;
});

test("navigation that exhausts the shared budget cannot start a report wait", async (t) => {
  let now = 0;
  t.mock.method(performance, "now", () => now);
  let timers = 0;
  t.mock.method(globalThis, "setTimeout", () => { timers += 1; assert.fail("No report budget remains"); });
  await assert.rejects(waitForFixtureReport({
    session: { async command() { now = 10000; return {}; } }, url: "http://fixture.invalid/controlled",
    result: new Promise(() => {}), exited: new Promise(() => {}), timeoutMs: 10000,
  }), /Fixture navigation exhausted its total time budget/);
  assert.equal(timers, 0);
});

test("a report already received during navigation succeeds without restarting the budget", async () => {
  const report = { status: "passed" };
  assert.equal(await waitForFixtureReport({
    session: { async command() { return {}; } }, url: "http://fixture.invalid/controlled",
    result: Promise.resolve(report), exited: new Promise(() => {}), timeoutMs: 10000,
  }), report);
});

test("navigation errors propagate before the report phase", async () => {
  const failure = new Error("Page.navigate transport failed");
  await assert.rejects(waitForFixtureReport({
    session: { async command() { throw failure; } }, url: "http://fixture.invalid/controlled",
    result: Promise.resolve({ status: "passed" }), exited: new Promise(() => {}), timeoutMs: 10000,
  }), (error) => {
    assert.equal(error, failure);
    return true;
  });
});

function viewportCommands(t) {
  const filename = path.join(__dirname, "helpers/runtime-browser-process.cjs");
  const source = fs.readFileSync(filename, "utf8");
  const declarations = new Map();
  visitSyntax(parseSource(source, filename), (node) => {
    if (ts.isFunctionDeclaration(node) && ["deadline", "openViewportSession"].includes(node.name?.text)) {
      declarations.set(node.name.text, sourceText(source, node));
      return false;
    }
  });
  assert.equal(declarations.size, 2, "Exercise the real command and deadline implementation");
  const timers = new Set();
  let socket;
  class Socket extends EventTarget {
    static OPEN = 1;
    readyState = Socket.OPEN;
    listeners = new Map();
    closes = 0;
    constructor() {
      super();
      socket = this;
      queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    addEventListener(type, listener, options) {
      const listeners = this.listeners.get(type) ?? new Set();
      listeners.add(listener); this.listeners.set(type, listeners);
      super.addEventListener(type, listener, options);
    }
    removeEventListener(type, listener, options) {
      this.listeners.get(type)?.delete(listener);
      super.removeEventListener(type, listener, options);
    }
    send(message) { this.sent = JSON.parse(message); }
    close() { this.closes += 1; this.readyState = 3; this.dispatchEvent(new Event("close")); }
    reply(result) { this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ id: this.sent.id, result }) })); }
    get listenerCount() { return [...this.listeners.values()].reduce((count, listeners) => count + listeners.size, 0); }
  }
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const open = vm.runInNewContext(`${[...declarations.values()].join("\n")}\nopenViewportSession;`, {
    assert, WebSocket: Socket,
    setTimeout(callback, milliseconds) {
      const timer = setTimeout(callback, milliseconds); timers.add(timer); return timer;
    },
    clearTimeout(timer) { timers.delete(timer); clearTimeout(timer); },
  }, { filename });
  return { open, timers, get socket() { return socket; } };
}

for (const method of ["Runtime.evaluate", "Emulation.setDeviceMetricsOverride"]) {
  test(`${method} retains the three-second CDP limit and releases command resources`, async (t) => {
    const transport = viewportCommands(t);
    const session = await transport.open("ws://controlled.invalid");
    assert.equal(transport.timers.size, 0);
    const acknowledged = session.command(method, {});
    transport.socket.reply({ acknowledged: true });
    assert.equal((await acknowledged).acknowledged, true);
    assert.equal(transport.timers.size, 0);
    assert.equal(transport.socket.listenerCount, 0);
    let settled = false;
    const command = session.command(method, {});
    command.then(() => { settled = true; }, () => { settled = true; });
    const rejected = assert.rejects(command, new RegExp(`${method} exceeded 3000 ms`));
    t.mock.timers.tick(2999);
    await Promise.resolve();
    assert.equal(settled, false);
    t.mock.timers.tick(1);
    await rejected;
    assert.equal(transport.socket.closes, 1);
    assert.equal(transport.socket.listenerCount, 0);
    assert.equal(transport.timers.size, 0);
  });
}
