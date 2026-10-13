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
  const modules = new Map([[entry.url, entry]]);
  const client = {
    _pendingRequests: new Map(),
    depsOptimizer: { scanProcessing: undefined, metadata: { depInfoList: [] } },
    moduleGraph: { async getModuleByUrl(url) { return modules.get(url); } },
    async transformRequest(url) { calls.push(["module", url]); return { code: "export {};" }; },
    async waitForRequestsIdle() { calls.push(["idle"]); }
  };
  return { calls, client, entry, modules, server: {
    config: { experimental: { bundledDev: false } },
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

test("bundled preparation waits for the initial build and rejects missing output", async (t) => {
  const { server, client, calls } = boundary();
  server.config.experimental.bundledDev = true;
  let release;
  const built = new Promise((resolve) => { release = resolve; });
  t.after(() => release());
  client.bundledDev = { waitForInitialBuildFinish: () => built, hasBuildOutput: true };
  let completed = false;
  const preparation = prepareFixtureServer(server, "runtime-browser.html", []);
  preparation.then(() => { completed = true; });
  await nextTurn();
  assert.equal(completed, false);
  release();
  await preparation;
  assert.deepEqual(calls, [["listen"]], "Bundled preparation must not use native ESM transforms");
  client.bundledDev.hasBuildOutput = false;
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", []), /must produce initial output/);
});

async function createBundledFixture(t, files) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-bundle-")));
  let server;
  t.after(async () => {
    try { await server?.close(); }
    finally { await fs.rm(root, { recursive: true, force: true }); }
  });
  const helpers = path.join(root, "tests/helpers");
  await fs.mkdir(helpers, { recursive: true });
  for (const [name, source] of Object.entries({
    "runtime-browser.html": '<script type="module" src="./entry.js"></script>',
    "unrelated.html": '<script type="module">this must not compile</script>',
    ...files,
  })) await fs.writeFile(path.join(helpers, name), source);
  const { createServer, createLogger } = await import("vite");
  const errors = [];
  const logger = createLogger("silent");
  logger.error = (message) => errors.push(String(message));
  server = await createServer({ configFile: false, root, cacheDir: path.join(root, "cache"),
    experimental: { bundledDev: true }, input: "tests/helpers/runtime-browser.html",
    appType: "mpa", logLevel: "silent", customLogger: logger,
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false } });
  return { server, errors };
}

test("real bundled Vite prepares only the selected entry with lazy, raw and CSS imports", { timeout: 15000 }, async (t) => {
  const { server, errors } = await createBundledFixture(t, {
    "entry.js": 'import note from "./note.md?raw"; import "./screen.css"; globalThis.fixture = { note, open: () => import("./lazy.js") };',
    "lazy.js": 'throw new Error("Preparation must not execute browser modules"); export const value = 1;',
    "note.md": "# Bundled markdown\n\nIt's **not JavaScript**.\n",
    "screen.css": "body { color: rgb(12, 34, 56); }",
  });
  await prepareFixtureServer(server, "runtime-browser.html", errors);
  assert.deepEqual(errors, []);
  assert.equal(server.environments.client.bundledDev.hasBuildOutput, true);
  const response = await fetch(`http://127.0.0.1:${server.httpServer.address().port}/tests/helpers/runtime-browser.html`);
  assert.equal(response.status, 200);
  assert.match(await response.text(), /<script/);
});

test("real bundled Vite compilation errors prevent readiness even after the build promise resolves", { timeout: 15000 }, async (t) => {
  const { server, errors } = await createBundledFixture(t, {
    "entry.js": 'import "./broken.js";',
    "broken.js": "export const invalid = ;",
  });
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", errors), /Fixture compilation failed:.*broken\.js/s);
  assert.ok(errors.length > 0, "The actual bundler must report its failed entry dependency");
});

test("preparation transforms reachable lazy modules through the real Vite graph without executing them", async (t) => {
  // Hosted Windows TEMP can use an 8.3 alias, which Vite's file-access boundary rejects.
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-browser-preparation-")));
  const helpers = path.join(root, "tests/helpers");
  await fs.mkdir(helpers, { recursive: true });
  for (const [name, source] of Object.entries({
    "runtime-browser.tsx": 'import note from "./note.md?raw"; export { note }; export const open = () => import("./lazy.js");',
    "lazy.js": 'import "./static.js"; import note from "./note.md?raw"; export { note }; export const nested = () => import("./nested.js");',
    "static.js": 'throw new Error("Preparation must not execute browser modules");',
    "nested.js": 'import "./lazy.js"; export const value = 1;',
    "note.md": "# Watched markdown\n\nIt's **not JavaScript**.\n",
    "unreachable.js": 'this is deliberately invalid and outside the entry graph',
  })) await fs.writeFile(path.join(helpers, name), source);
  const { createServer } = await import("vite");
  const server = await createServer({ configFile: false, root, cacheDir: path.join(root, "cache"),
    appType: "mpa", optimizeDeps: { noDiscovery: true, include: [] }, logLevel: "error",
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false, preTransformRequests: false } });
  t.after(async () => { await server.close(); await fs.rm(root, { recursive: true, force: true }); });
  await prepareFixtureServer(server, "runtime-browser.html", []);
  for (const name of ["runtime-browser.tsx", "lazy.js", "static.js", "nested.js", "note.md?raw"]) {
    const module = await server.environments.client.moduleGraph.getModuleByUrl(`/tests/helpers/${name}`);
    assert.ok(module?.transformResult, `${name} must be prepared before browser launch`);
  }
  const watched = await server.environments.client.moduleGraph.getModuleByUrl("/tests/helpers/note.md");
  assert.equal(watched?.type, "js", "Vite records this raw source as a JavaScript-shaped watch dependency");
  assert.equal(watched.transformResult, null, "The raw file must never be transformed as JavaScript");
  assert.equal(await server.environments.client.moduleGraph.getModuleByUrl("/tests/helpers/unreachable.js"), undefined);
});

test("lazy preparation bounds its transform wave and handles cycles and watch-only dependencies", async () => {
  const { server, client, entry, modules } = boundary();
  const lazy = Array.from({ length: 9 }, (_, index) => ({ url: `/lazy-${index}.js`, type: "js",
    transformResult: null, importedModules: new Set([entry]) }));
  const asset = { url: "/watched-partial.css", type: "asset", transformResult: null, importedModules: new Set() };
  entry.importedModules = new Set([...lazy, asset]);
  entry.transformResult.code = lazy.map((module, index) => `export const load${index} = () => import("${module.url}");`).join("\n");
  for (const module of lazy) modules.set(module.url, module);
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
    return module.transformResult = { code: `import "${entry.url}";` };
  };
  await prepareFixtureServer(server, "runtime-browser.html", []);
  assert.equal(peak, 4);
  assert.deepEqual(transformed.sort(), lazy.map((module) => module.url).sort());
});

test("a lazy module transform failure prevents readiness", async () => {
  const { server, client, entry, modules } = boundary();
  const lazy = { url: "/broken-lazy.js", type: "js", transformResult: null, importedModules: new Set() };
  entry.importedModules.add(lazy);
  entry.transformResult.code = 'export const open = () => import("/broken-lazy.js");';
  modules.set(lazy.url, lazy);
  const failure = new Error("Lazy module could not compile");
  client.transformRequest = async (url) => {
    if (url === lazy.url) throw failure;
    return entry.transformResult;
  };
  await assert.rejects(prepareFixtureServer(server, "runtime-browser.html", []), (error) => error === failure);
});
