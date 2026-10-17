const assert = require("node:assert/strict");
const test = require("node:test");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { browserStartupBudget, browserCommand, openViewportSession, waitForBrowserEndpoint,
  browserTimeBudgets } = require("./helpers/runtime-browser-process.cjs");

function controlledTransport(t) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let now = 0;
  t.mock.method(performance, "now", () => now);
  const timers = new Set();
  const schedule = globalThis.setTimeout;
  const cancel = globalThis.clearTimeout;
  t.mock.method(globalThis, "setTimeout", (callback, milliseconds) => {
    const timer = schedule(() => { timers.delete(timer); callback(); }, milliseconds);
    timers.add(timer); return timer;
  });
  t.mock.method(globalThis, "clearTimeout", (timer) => { timers.delete(timer); cancel(timer); });
  const sockets = [];
  class Socket extends EventTarget {
    static OPEN = 1;
    readyState = 0;
    closes = 0;
    sent = [];
    listeners = new Map();
    constructor() { super(); sockets.push(this); }
    addEventListener(type, listener, options) {
      const listeners = this.listeners.get(type) ?? new Set();
      listeners.add(listener); this.listeners.set(type, listeners);
      super.addEventListener(type, listener, options);
    }
    removeEventListener(type, listener, options) {
      this.listeners.get(type)?.delete(listener);
      super.removeEventListener(type, listener, options);
    }
    open() { this.readyState = Socket.OPEN; this.dispatchEvent(new Event("open")); }
    send(message) { this.sent.push(JSON.parse(message)); }
    reply(result) {
      this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ id: this.sent.at(-1).id, result }) }));
    }
    close() { this.closes++; this.readyState = 3; this.dispatchEvent(new Event("close")); }
    get listenerCount() { return [...this.listeners.values()].reduce((count, listeners) => count + listeners.size, 0); }
  }
  t.mock.method(globalThis, "WebSocket", function () { return new Socket(); });
  return { sockets, timers, get now() { return now; },
    async advance(milliseconds) { now += milliseconds; t.mock.timers.tick(milliseconds); await nextTurn(); } };
}

test("endpoint, cold CDP handshake and viewport setup consume one startup deadline", async (t) => {
  const transport = controlledTransport(t);
  const budget = browserStartupBudget(0, 10000, 80000, new Promise(() => {}));
  t.after(() => budget.dispose());
  let publish;
  const endpoint = waitForBrowserEndpoint(new Promise((resolve) => { publish = resolve; }),
    new Promise(() => {}), budget.remaining());
  await transport.advance(1349);
  publish("ws://controlled.invalid/browser");
  assert.equal(await endpoint, "ws://controlled.invalid/browser");
  assert.equal(budget.remaining(), 8651);
  const target = browserCommand(await endpoint, "Target.createTarget", { url: "about:blank" }, budget.remaining(), budget.signal);
  const browserSocket = transport.sockets.at(-1);
  await transport.advance(3500);
  assert.equal(browserSocket.sent.length, 0, "A cold handshake beyond three seconds is still inside startup");
  browserSocket.open(); browserSocket.reply({ targetId: "owned-target" });
  assert.deepEqual(await target, { targetId: "owned-target" });
  assert.equal(browserSocket.closes, 1);
  assert.equal(browserSocket.listenerCount, 0);
  assert.equal(budget.remaining(), 5151);

  await transport.advance(1000); // Time used to discover the exact owned page.
  const opening = openViewportSession("ws://controlled.invalid/owned-target", budget.remaining(), budget.signal);
  const viewportSocket = transport.sockets.at(-1);
  await transport.advance(1000); viewportSocket.open();
  const session = await opening;
  assert.equal(budget.remaining(), 3151);
  const metrics = session.command("Emulation.setDeviceMetricsOverride", {}, budget.remaining(), budget.signal);
  await transport.advance(2000); viewportSocket.reply({}); await metrics;
  assert.equal(budget.remaining(), 1151, "Viewport setup cannot restart the startup clock");
  const inspected = session.command("Runtime.evaluate", {}, budget.remaining(), budget.signal);
  await transport.advance(1150); viewportSocket.reply({ result: { value: "ready" } }); await inspected;
  assert.equal(budget.remaining(), 1);
  await transport.advance(1);
  assert.throws(() => budget.remaining(), /startup exhausted its total time budget/);
  session.close();
  assert.equal(viewportSocket.listenerCount, 0);
  assert.equal(transport.timers.size, 0);
});

test("startup is clamped to the existing command cleanup boundary and ignores exit after disposal", async (t) => {
  const transport = controlledTransport(t);
  let exit;
  const budget = browserStartupBudget(0, 30000, 12000, new Promise((resolve) => { exit = resolve; }));
  assert.equal(budget.remaining(), 12000);
  await transport.advance(11000);
  assert.equal(budget.remaining(), 1000);
  budget.dispose(); exit({ code: 0 }); await nextTurn();
  assert.equal(budget.signal.aborted, false, "A normal later browser exit cannot cancel completed startup");
  const limits = browserTimeBudgets({ browserStartupTimeoutMs: 30000 });
  assert.equal(limits.commandTimeoutMs, 80000);
  assert.equal(limits.fixtureTimeoutMs, 45000);
});

test("an expired startup phase cannot create a socket or obtain another deadline", async (t) => {
  const transport = controlledTransport(t);
  const budget = browserStartupBudget(0, 1000, 80000, new Promise(() => {}));
  t.after(() => budget.dispose());
  await transport.advance(1000);
  assert.throws(() => browserCommand("ws://controlled.invalid", "Target.createTarget", {}, budget.remaining(), budget.signal),
    /startup exhausted its total time budget/);
  assert.equal(transport.sockets.length, 0);
  await assert.rejects(browserCommand("ws://controlled.invalid", "Target.createTarget", {}, 0), /remaining time budget/);
  assert.equal(transport.sockets.length, 0);
  assert.equal(transport.timers.size, 0);
});

test("a pending cold handshake expires at the remaining startup deadline and releases resources", async (t) => {
  const transport = controlledTransport(t);
  const budget = browserStartupBudget(0, 5000, 80000, new Promise(() => {}));
  t.after(() => budget.dispose());
  await transport.advance(1000);
  const pending = browserCommand("ws://controlled.invalid", "Target.createTarget", {}, budget.remaining(), budget.signal);
  const rejected = assert.rejects(pending, /Target.createTarget exceeded 4000 ms; CDP progress:/);
  await transport.advance(3999);
  assert.equal(transport.sockets[0].closes, 0);
  await transport.advance(1); await rejected;
  assert.throws(() => budget.remaining(), /startup exhausted its total time budget/);
  assert.equal(transport.sockets[0].closes, 1);
  assert.equal(transport.sockets[0].listenerCount, 0);
  assert.equal(transport.timers.size, 0);
});

for (const opened of [false, true]) test(`CDP close ${opened ? "after" : "before"} open rejects immediately without waiting for a timeout`, async (t) => {
  const transport = controlledTransport(t);
  const pending = browserCommand("ws://controlled.invalid", "Target.createTarget");
  const rejected = assert.rejects(pending, /Target.createTarget connection closed before response/);
  if (opened) transport.sockets[0].open();
  transport.sockets[0].dispatchEvent(new Event("close"));
  await rejected;
  assert.equal(transport.now, 0);
  assert.equal(transport.sockets[0].closes, 1);
  assert.equal(transport.sockets[0].listenerCount, 0);
  assert.equal(transport.timers.size, 0);
});

for (const phase of ["target", "viewport connection", "viewport command"]) test(`browser exit cancels pending ${phase} and releases its socket`, async (t) => {
  const transport = controlledTransport(t);
  let exit;
  const budget = browserStartupBudget(0, 30000, 80000, new Promise((resolve) => { exit = resolve; }));
  t.after(() => budget.dispose());
  let pending;
  if (phase === "target") pending = browserCommand("ws://controlled.invalid", "Target.createTarget", {}, budget.remaining(), budget.signal);
  else {
    pending = openViewportSession("ws://controlled.invalid", budget.remaining(), budget.signal);
    if (phase === "viewport command") {
      transport.sockets[0].open();
      const session = await pending;
      pending = session.command("Runtime.evaluate", {}, budget.remaining(), budget.signal);
    }
  }
  const rejected = assert.rejects(pending, /Browser exited during startup: \{"code":7\}/);
  exit({ code: 7 }); await rejected;
  assert.equal(transport.sockets[0].closes, 1);
  assert.equal(transport.sockets[0].listenerCount, 0);
  assert.equal(transport.timers.size, 0);
  assert.throws(() => budget.remaining(), /Browser exited during startup/);
  await assert.rejects(browserCommand("ws://controlled.invalid", "Target.createTarget", {}, 1000, budget.signal),
    /Browser exited during startup/);
  assert.equal(transport.sockets.length, 1, "An exited browser must not receive another connection");
});
