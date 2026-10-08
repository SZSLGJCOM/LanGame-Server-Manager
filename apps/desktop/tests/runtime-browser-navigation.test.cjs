const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
const { parseSource, sourceText, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

function heldDocument(t, release, milliseconds) {
  let requests = 0;
  const timers = new Set();
  t.after(() => { for (const timer of timers) clearTimeout(timer); });
  return {
    get requests() { return requests; },
    middleware(request, response, next) {
      if (request.url?.split("?")[0] !== "/tests/helpers/runtime-browser.html") return next();
      requests += 1;
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

function verifiedFailureCleanup(error) {
  assert.ok(error instanceof AggregateError);
  assert.equal(error.errors.length, 1, "A fixture timeout must not introduce cleanup failures");
  assert.equal(error.browserExited, true);
  assert.equal(error.retainedScratch, null, "Verified tree exit must release the owned profile");
  assert.throws(() => process.kill(error.browserPid, 0), { code: "ESRCH" });
}

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

test("navigation that never receives a document expires within the fixture budget and releases its tree", { timeout: 30000 }, async (t) => {
  const document = heldDocument(t, () => assert.fail("The held document must not be released"), null);
  await assert.rejects(runBrowserFixture({ viewport: { width: 960, height: 600 }, fixtureTimeoutMs: 1000,
    fixtureMiddleware: document.middleware }), (error) => {
    verifiedFailureCleanup(error);
    const timeout = error.errors[0].message.match(/^Page\.navigate exceeded (\d+) ms$/);
    assert.ok(timeout, error.errors[0].message);
    assert.ok(Number(timeout[1]) > 0 && Number(timeout[1]) <= 1000, "Navigation must use the fixture's remaining budget");
    return true;
  });
  assert.equal(document.requests, 1);
});

test("the report cannot obtain a fresh budget after navigation consumes part of it", { timeout: 30000 }, async (t) => {
  const document = heldDocument(t, (_request, response) => {
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    response.end(`<!doctype html><html><body><script>
      setTimeout(() => fetch('/__reliability_result/' + new URLSearchParams(location.search).get('nonce'), {
        method: 'POST', body: JSON.stringify({ status: 'passed', browser_errors: [] })
      }), 7000);
    </script></body></html>`);
  }, 3500);
  await assert.rejects(runBrowserFixture({ viewport: { width: 960, height: 600 }, fixtureTimeoutMs: 10000,
    fixtureMiddleware: document.middleware }), (error) => {
    verifiedFailureCleanup(error);
    assert.match(error.errors[0].message, /^Real ReactDOM reliability fixture exceeded \d+(?:\.\d+)? ms$/);
    return true;
  });
  assert.equal(document.requests, 1);
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
