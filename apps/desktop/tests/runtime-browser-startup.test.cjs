const assert = require("node:assert/strict");
const test = require("node:test");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { browserStartupBudget, browserCommand, openViewportSession, waitForBrowserEndpoint,
  browserTimeBudgets, browserNetworkDiagnostics } = require("./helpers/runtime-browser-process.cjs");

test("network failures retain their resource and browser cause without request secrets", () => {
  const diagnostics = browserNetworkDiagnostics("http://127.0.0.1:43210/fixture?nonce=secret");
  diagnostics.observe({ method: "Network.requestWillBeSent", params: { requestId: "script", type: "Script",
    request: { url: "http://user:secret@127.0.0.1:43210/assets/api-mock.js?token=secret#secret",
      headers: { Authorization: "secret" }, postData: "secret" } } });
  diagnostics.observe({ method: "Network.responseReceived", params: { requestId: "script", response: {
    status: 200, headers: { "Set-Cookie": "secret" } } } });
  diagnostics.observe({ method: "Network.loadingFailed", params: { requestId: "script", errorText: "net::ERR_CONNECTION_RESET",
    canceled: false, blockedReason: "cors", corsErrorStatus: { corsError: "InvalidAllowOriginValue", failedParameter: "secret" } } });
  assert.deepEqual(diagnostics.snapshot(), { failures: [{ path: "/assets/api-mock.js", type: "Script", status: 200,
    error: "net::ERR_CONNECTION_RESET", canceled: false, blocked_reason: "cors", cors_error: "InvalidAllowOriginValue" }],
    dropped_requests: 0, dropped_failures: 0, observation_errors: 0 });
  const snapshot = diagnostics.snapshot();
  snapshot.failures[0].path = "mutated";
  assert.equal(diagnostics.snapshot().failures[0].path, "/assets/api-mock.js");
  assert.equal(JSON.stringify(diagnostics.snapshot()).includes("secret"), false);
});

test("network observations release completed and redirected requests and bound retained failures", () => {
  const diagnostics = browserNetworkDiagnostics("http://127.0.0.1:43210/fixture");
  const request = (requestId, url = `http://127.0.0.1:43210/${requestId}.js`) => diagnostics.observe({
    method: "Network.requestWillBeSent", params: { requestId, type: "Script", request: { url } } });
  const fail = (requestId) => diagnostics.observe({ method: "Network.loadingFailed",
    params: { requestId, errorText: "net::ERR_FAILED", canceled: true } });
  request("complete");
  diagnostics.observe({ method: "Network.loadingFinished", params: { requestId: "complete" } });
  request("redirected"); request("redirected", "https://external.invalid/secret");
  fail("complete"); fail("redirected");
  assert.equal(diagnostics.snapshot().failures.length, 0);
  for (let id = 0; id < 257; id++) request(String(id));
  for (let id = 0; id < 257; id++) fail(String(id));
  const result = diagnostics.snapshot();
  assert.equal(result.dropped_requests, 1);
  assert.equal(result.dropped_failures, 240);
  assert.equal(result.failures.length, 16);
  assert.equal(result.failures[0].path, "/241.js");
  assert.equal(result.failures.at(-1).path, "/256.js");
  request("long", `http://127.0.0.1:43210/${"a".repeat(1000)}?secret`); fail("long");
  assert.equal(diagnostics.snapshot().failures.at(-1).path.length, 500);
});

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

for (const timeout of [false, true]) test(`network diagnostics release their listener on ${timeout ? "command timeout" : "normal close"}`, async (t) => {
  const transport = controlledTransport(t);
  const diagnostics = browserNetworkDiagnostics("http://127.0.0.1:43210/fixture");
  const opening = openViewportSession("ws://controlled.invalid/owned-target", 1000, undefined, diagnostics);
  const socket = transport.sockets[0]; socket.open();
  const session = await opening;
  assert.equal(session.isOpen(), true);
  const enabling = session.command("Network.enable", {}, 1000);
  socket.reply({}); await enabling;
  socket.dispatchEvent(new MessageEvent("message", { data: "invalid diagnostic packet" }));
  assert.equal(diagnostics.snapshot().observation_errors, 1, "Observer errors must not replace the fixture result");
  const command = session.command("Runtime.evaluate", {}, 500);
  if (timeout) {
    const rejected = assert.rejects(command, /Runtime.evaluate exceeded 500 ms/);
    await transport.advance(500); await rejected;
  } else {
    for (const event of [{ method: "Network.requestWillBeSent", params: { requestId: "module", type: "Script",
      request: { url: "http://127.0.0.1:43210/assets/module.js?secret" } } },
    { method: "Network.loadingFailed", params: { requestId: "module", errorText: "net::ERR_EMPTY_RESPONSE" } }]) {
      socket.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(event) }));
    }
    socket.reply({ result: { value: "ready" } });
    assert.deepEqual(await command, { result: { value: "ready" } });
    assert.equal(diagnostics.snapshot().failures[0].error, "net::ERR_EMPTY_RESPONSE",
      "Inspection through the same session must observe network events preceding its reply");
    session.close();
  }
  assert.equal(session.isOpen(), false);
  assert.equal(socket.closes, 1);
  assert.equal(socket.listenerCount, 0);
  assert.equal(transport.timers.size, 0);
});

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
