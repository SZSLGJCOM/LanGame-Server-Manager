const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadSource(relativePath, dependencies = {}) {
  const filename = path.join(__dirname, "../src", relativePath);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, Error,
    require: (name) => {
      assert.ok(Object.hasOwn(dependencies, name), `Unexpected dependency: ${name}`);
      return dependencies[name];
    }
  }, { filename });
  return exports;
}

const { createDeferredModule } = loadSource("deferred-module.ts");
const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((accept, decline) => { resolve = accept; reject = decline; });
  return { promise, resolve, reject };
}

function hookHarness() {
  const state = [];
  const effects = [];
  let stateIndex = 0;
  let effectIndex = 0;
  let updates = 0;
  const react = {
    useCallback: (callback) => callback,
    useState(initial) {
      const position = stateIndex++;
      if (!(position in state)) state[position] = typeof initial === "function" ? initial() : initial;
      return [state[position], (next) => {
        updates++;
        state[position] = typeof next === "function" ? next(state[position]) : next;
      }];
    },
    useEffect(callback, dependencies) {
      const position = effectIndex++;
      const previous = effects[position];
      if (!previous || dependencies.some((value, index) => value !== previous.dependencies[index])) {
        effects[position] = { callback, dependencies, cleanup: previous?.cleanup, pending: true };
      }
    }
  };
  const { useDeferredModule } = loadSource("hooks/useDeferredModule.ts", { react });
  return {
    render(source, enabled = true) {
      stateIndex = 0;
      effectIndex = 0;
      return useDeferredModule(source, enabled);
    },
    commit() {
      for (const effect of effects) {
        if (!effect.pending) continue;
        effect.pending = false;
        effect.cleanup?.();
        effect.cleanup = effect.callback();
      }
    },
    unmount() { for (const effect of effects) effect.cleanup?.(); },
    updates: () => updates
  };
}

test("concurrent preloads and opens share a flight and reuse the successful module", async () => {
  const flight = deferred();
  const component = () => null;
  let reads = 0;
  const source = createDeferredModule(() => { reads++; return flight.promise; });
  const first = source.load();
  assert.equal(source.load(), first);
  assert.equal(source.peek(), null);
  await flush();
  assert.equal(reads, 1);
  flight.resolve(component);
  assert.equal(await first, component);
  assert.equal(source.peek(), component);
  assert.equal(await source.load(), component);
  assert.equal(reads, 1);
});

test("a failed import is retained until an explicit recovery entry is requested", async () => {
  let reads = 0;
  const component = () => null;
  const source = createDeferredModule(
    () => { reads++; throw new Error("Module request failed"); },
    () => { reads++; return Promise.resolve(component); }
  );
  await assert.rejects(source.load(), /Module request failed/);
  assert.equal(source.peek(), null);
  assert.equal(source.canRetry(), true);
  await assert.rejects(source.load(), /Module request failed/);
  assert.equal(reads, 1, "hover or opening after a failed preload must not consume the recovery entry");
  assert.equal(await source.retry(), component);
  assert.equal(reads, 2);
});

test("failed recovery is bounded and cannot advertise another ineffective retry", async () => {
  let reads = 0;
  const loader = async () => { reads++; throw new Error("Shared dependency unavailable"); };
  const source = createDeferredModule(loader, loader);
  await assert.rejects(source.load(), /Shared dependency unavailable/);
  await assert.rejects(source.retry(), /Shared dependency unavailable/);
  assert.equal(source.canRetry(), false);
  await assert.rejects(source.retry(), /Shared dependency unavailable/);
  await assert.rejects(source.load(), /Shared dependency unavailable/);
  assert.equal(reads, 2);
});

test("opening reports loading before effects and retry clears the failed view immediately", async () => {
  const flights = [deferred(), deferred()];
  let reads = 0;
  const source = createDeferredModule(() => flights[reads++].promise, () => flights[reads++].promise);
  const hook = hookHarness();
  assert.equal(hook.render(source).status, "loading");
  assert.equal(reads, 0);
  hook.commit();
  await flush();
  flights[0].reject(new Error("Offline"));
  await flush();
  const failed = hook.render(source);
  assert.equal(failed.status, "error");
  assert.equal(failed.error.message, "Offline");
  assert.equal(failed.canRetry, true);
  failed.retry();
  assert.equal(hook.render(source).status, "loading");
  hook.commit();
  await flush();
  const component = () => null;
  flights[1].resolve(component);
  await flush();
  assert.equal(hook.render(source).value, component);
  assert.equal(reads, 2);
});

test("disabled and unmounted consumers ignore late failures", async () => {
  const flight = deferred();
  const source = createDeferredModule(() => flight.promise);
  const hook = hookHarness();
  assert.equal(hook.render(source, false).status, "idle");
  hook.commit();
  hook.render(source);
  hook.commit();
  await flush();
  assert.equal(hook.render(source, false).status, "idle");
  hook.commit();
  hook.unmount();
  const before = hook.updates();
  flight.reject(new Error("Late failure"));
  await flush();
  assert.equal(hook.updates(), before);
});

test("switching modules cannot replace the current selection with a late result", async () => {
  const first = deferred();
  const second = deferred();
  const firstSource = createDeferredModule(() => first.promise);
  const secondSource = createDeferredModule(() => second.promise);
  const hook = hookHarness();
  hook.render(firstSource);
  hook.commit();
  await flush();
  assert.equal(hook.render(secondSource).status, "loading");
  hook.commit();
  await flush();
  first.resolve(() => "old");
  await flush();
  assert.equal(hook.render(secondSource).status, "loading");
  const selected = () => "current";
  second.resolve(selected);
  await flush();
  assert.equal(hook.render(secondSource).value, selected);
  assert.equal(hook.render(firstSource).status, "ready", "completed inactive loads remain reusable");
});
