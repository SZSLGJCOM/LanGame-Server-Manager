const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const flush = () => new Promise((resolve) => setImmediate(resolve));
const details = (id) => ({ summary: { id }, schema_json: "{}" });

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness(readModuleDetails) {
  const filename = path.join(__dirname, "../src/hooks/useDesktopEffects.ts");
  const exports = {};
  let dependencies;
  let cleanup;
  let state;
  const injected = {
    react: { useEffect(effect, nextDependencies) {
      if (dependencies && dependencies.every((item, index) => item === nextDependencies[index])) return;
      cleanup?.();
      dependencies = nextDependencies;
      cleanup = effect();
    } },
    "../api": { readModuleDetails },
    "../app-state": { describeError: (error) => error instanceof Error ? error.message : String(error) },
    "../instance-panel-loader": {},
    "../bootstrap-initialization": { createBootstrapInitializationCoordinator: () => ({}) },
    "../domain/single-flight-poller": {},
    "../domain/system-resources": {}
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require(id) {
      assert.ok(Object.hasOwn(injected, id), `unexpected dependency ${id}`);
      return injected[id];
    }
  }, { filename });
  return {
    get state() { return state; },
    render(selectedModuleId = "rune", retryGeneration = 0, enabled = true) {
      exports.useSelectedInstanceModuleDetailsSync({
        enabled, selectedModuleId, retryGeneration,
        onLoaded(value, error) { state = { details: value, error: error ?? null }; }
      });
    },
    dispose() { cleanup?.(); }
  };
}

test("a failed selected module read reports its error and explicit retry reads the same module", async () => {
  const reads = [];
  const retry = deferred();
  const reader = harness((id) => {
    reads.push(id);
    return reads.length === 1 ? Promise.reject(new Error("archive inventory is busy")) : retry.promise;
  });
  reader.render();
  await flush();
  assert.equal(reader.state.details, null);
  assert.equal(reader.state.error?.moduleId, "rune");
  assert.equal(reader.state.error?.message, "archive inventory is busy");
  reader.render();
  await flush();
  assert.equal(reads.length, 1, "a failure must not start automatic retries");
  reader.render("rune", 1);
  assert.equal(reader.state.error, null, "explicit retry enters loading instead of retaining the failure");
  await flush();
  assert.deepEqual(reads, ["rune", "rune"]);
  retry.resolve(details("rune"));
  await flush();
  assert.equal(reader.state.details.summary.id, "rune");
  assert.equal(reader.state.error, null);
  reader.dispose();
});

test("selection changes ignore old read success and failure while loading the current module", async () => {
  for (const outcome of ["resolve", "reject"]) {
    const first = deferred();
    const second = deferred();
    const reader = harness((id) => id === "rune" ? first.promise : second.promise);
    reader.render();
    await flush();
    reader.render("barotrauma");
    assert.equal(reader.state.details, null);
    assert.equal(reader.state.error, null);
    await flush();
    second.resolve(details("barotrauma"));
    await flush();
    first[outcome](outcome === "resolve" ? details("rune") : new Error("old request failed"));
    await flush();
    assert.equal(reader.state.details.summary.id, "barotrauma");
    assert.equal(reader.state.error, null);
    reader.dispose();
  }
});

test("leaving the server view clears an error and discards any pending read", async () => {
  const pending = deferred();
  const reader = harness(() => pending.promise);
  reader.render();
  await flush();
  reader.render("rune", 0, false);
  pending.reject(new Error("late native error"));
  await flush();
  assert.equal(reader.state.details, null);
  assert.equal(reader.state.error, null);
  reader.dispose();
});

test("a synchronous invocation failure reaches the selected module error state", async () => {
  const reader = harness(() => { throw new Error("native invocation unavailable"); });
  reader.render();
  await flush();
  assert.equal(reader.state.error?.message, "native invocation unavailable");
  reader.dispose();
});

test("first configuration read does not depend on archive inventory statistics", async () => {
  const requests = [];
  const reader = harness((id, options) => {
    requests.push({ id, options });
    if (options?.includePreservedProgramCounts !== false) {
      return Promise.reject(new Error("archive inventory is busy"));
    }
    return Promise.resolve(details(id));
  });
  reader.render();
  await flush();
  assert.equal(reader.state.error, null);
  assert.equal(reader.state.details?.summary.id, "rune");
  assert.equal(requests.length, 1, "the initial request succeeds without a retry");
  reader.dispose();
});
