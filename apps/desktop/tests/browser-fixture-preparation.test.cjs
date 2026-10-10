const assert = require("node:assert/strict");
const test = require("node:test");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { prepareFixtureServer } = require("./helpers/runtime-browser-process.cjs");

function boundary() {
  const calls = [];
  const client = {
    _pendingRequests: new Map(),
    depsOptimizer: { scanProcessing: undefined, metadata: { depInfoList: [] } },
    async transformRequest(url) { calls.push(["module", url]); return { code: "export {};" }; },
    async waitForRequestsIdle() { calls.push(["idle"]); }
  };
  return { calls, client, server: {
    environments: { client },
    async listen() { calls.push(["listen"]); },
    async transformIndexHtml(url, html) {
      calls.push(["html", url]);
      assert.match(html, /<script/);
      return '<script type="module" src="/tests/helpers/cold-entry.tsx"></script>';
    }
  } };
}

test("a listening server is not ready while its initial dependency bundle is pending", async (t) => {
  const { server, client, calls } = boundary();
  let release;
  const processing = new Promise((resolve) => { release = resolve; });
  client.depsOptimizer.metadata.depInfoList = [{ id: "react", processing }];
  let entryReached;
  const entered = new Promise((resolve) => { entryReached = resolve; });
  const transform = client.transformRequest;
  client.transformRequest = async (url) => { entryReached(); return transform(url); };
  let completed = false;
  const preparation = prepareFixtureServer(server, "runtime-browser.html", []);
  preparation.then(() => { completed = true; });
  t.after(() => release());
  assert.equal(await Promise.race([entered.then(() => true), preparation.then(() => false)]), true,
    "the actual entry transform must run before readiness can finish");
  await nextTurn();
  assert.equal(completed, false, "consumer must not launch before dependency processing finishes");
  client.depsOptimizer.metadata.depInfoList[0].processing = undefined;
  release();
  await preparation;
  assert.deepEqual(calls.slice(0, 3), [["listen"], ["html", "/tests/helpers/runtime-browser.html"], ["module", "/tests/helpers/cold-entry.tsx"]]);
});

test("an entry transform failure prevents launching a browser fixture", async () => {
  const { server, client } = boundary();
  const failure = new Error("Entry module could not compile");
  client.transformRequest = async () => { throw failure; };
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", []), (error) => error === failure);
});

test("resolved dependency promises do not turn a Vite compilation error into readiness", async () => {
  const { server, client } = boundary();
  const errors = [];
  client.transformRequest = async () => { errors.push("Dependency bundle could not compile"); return { code: "export {};" }; };
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", errors), /Dependency bundle could not compile/);
});

test("preparation drains transforms discovered after the initial transform wave", async (t) => {
  const { server, client } = boundary();
  let finishFirst;
  let finishSecond;
  let enter;
  const entered = new Promise((resolve) => { enter = resolve; });
  const second = new Promise((resolve) => { finishSecond = resolve; });
  const first = new Promise((resolve) => { finishFirst = resolve; }).then(() => {
    client._pendingRequests.delete("first");
    client._pendingRequests.set("second", { request: second });
  });
  client.transformRequest = async () => {
    client._pendingRequests.set("first", { request: first });
    enter();
    return { code: "export {};" };
  };
  let completed = false;
  const preparation = prepareFixtureServer(server, "runtime-browser.html", []);
  preparation.then(() => { completed = true; });
  t.after(() => { finishFirst(); finishSecond(); });
  await entered;
  finishFirst();
  await first;
  await nextTurn();
  assert.equal(completed, false, "late imports must still hold the preparation boundary");
  client._pendingRequests.delete("second");
  finishSecond();
  await preparation;
});

test("a missing transformed entry fails before browser launch", async () => {
  const { server, client } = boundary();
  client.transformRequest = async () => null;
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", []), /Fixture entry did not transform/);
});
