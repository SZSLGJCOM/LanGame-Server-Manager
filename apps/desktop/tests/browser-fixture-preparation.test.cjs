const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { prepareFixtureServer } = require("./helpers/runtime-browser-process.cjs");

function boundary() {
  const calls = [];
  const entry = { url: "/tests/helpers/cold-entry.tsx", type: "js",
    transformResult: { code: "export {};" }, importedModules: new Set() };
  const client = {
    _pendingRequests: new Map(),
    depsOptimizer: { scanProcessing: undefined, metadata: { depInfoList: [] } },
    moduleGraph: { async getModuleByUrl() { return entry; } },
    async transformRequest(url) { calls.push(["module", url]); return { code: "export {};" }; },
    async waitForRequestsIdle() { calls.push(["idle"]); }
  };
  return { calls, client, entry, server: {
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

test("preparation transforms reachable lazy modules through the real Vite graph without executing them", async (t) => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-preparation-"));
  const helpers = path.join(root, "tests/helpers");
  await fs.mkdir(helpers, { recursive: true });
  for (const [name, source] of Object.entries({
    "runtime-browser.tsx": 'export const open = () => import("./lazy.js");',
    "lazy.js": 'import "./static.js"; export const nested = () => import("./nested.js");',
    "static.js": 'throw new Error("Preparation must not execute browser modules");',
    "nested.js": 'import "./lazy.js"; export const value = 1;',
    "unreachable.js": 'this is deliberately invalid and outside the entry graph',
  })) await fs.writeFile(path.join(helpers, name), source);
  const { createServer } = await import("vite");
  const server = await createServer({ configFile: false, root, cacheDir: path.join(root, "cache"),
    appType: "mpa", optimizeDeps: { noDiscovery: true, include: [] }, logLevel: "error",
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false } });
  t.after(async () => { await server.close(); await fs.rm(root, { recursive: true, force: true }); });
  await prepareFixtureServer(server, "runtime-browser.html", []);
  for (const name of ["runtime-browser.tsx", "lazy.js", "static.js", "nested.js"]) {
    const module = await server.environments.client.moduleGraph.getModuleByUrl(`/tests/helpers/${name}`);
    assert.ok(module?.transformResult, `${name} must be prepared before browser launch`);
  }
  assert.equal(await server.environments.client.moduleGraph.getModuleByUrl("/tests/helpers/unreachable.js"), undefined);
});

test("lazy preparation bounds its transform wave and handles cycles and CSS watch-only assets", async () => {
  const { server, client, entry } = boundary();
  const lazy = Array.from({ length: 9 }, (_, index) => ({ url: `/lazy-${index}.js`, type: "js",
    transformResult: null, importedModules: new Set([entry]) }));
  const asset = { url: "/watched-partial.css", type: "asset", transformResult: null, importedModules: new Set() };
  entry.importedModules = new Set([...lazy, asset]);
  const transformed = [];
  let active = 0;
  let peak = 0;
  client.transformRequest = async (url) => {
    if (url === entry.url) return entry.transformResult;
    const module = lazy.find((item) => item.url === url);
    assert.ok(module, "File-only CSS dependencies must not be requested as modules");
    active++;
    peak = Math.max(peak, active);
    await nextTurn();
    active--;
    transformed.push(url);
    return module.transformResult = { code: "export {};" };
  };
  await prepareFixtureServer(server, "runtime-browser.html", []);
  assert.equal(peak, 4);
  assert.deepEqual(transformed.sort(), lazy.map((module) => module.url).sort());
});

test("a lazy module transform failure prevents readiness", async () => {
  const { server, client, entry } = boundary();
  const lazy = { url: "/broken-lazy.js", type: "js", transformResult: null, importedModules: new Set() };
  entry.importedModules.add(lazy);
  const failure = new Error("Lazy module could not compile");
  client.transformRequest = async (url) => {
    if (url === lazy.url) throw failure;
    return entry.transformResult;
  };
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", []), (error) => error === failure);
});
