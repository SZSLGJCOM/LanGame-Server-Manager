const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const filename = path.resolve(__dirname, "../src/hooks/useAssistantPromptRotation.ts");
const source = transpileTypeScript(fs.readFileSync(filename, "utf8"), filename);
const prompt = (id, label = id) => ({ id, label });

function eventTarget(properties) {
  const listeners = new Map();
  return {
    ...properties,
    addEventListener(type, callback) {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type).add(callback);
    },
    removeEventListener(type, callback) { listeners.get(type)?.delete(callback); },
    dispatch(type) { listeners.get(type)?.forEach((callback) => callback()); },
    listenerCount() { return [...listeners.values()].reduce((total, entries) => total + entries.size, 0); }
  };
}

function rotationHarness(prompts, options = {}) {
  const state = [];
  const effects = [];
  const timers = new Map();
  const document = eventTarget({ hidden: options.hidden ?? false });
  const media = eventTarget({ matches: options.reducedMotion ?? false });
  let stateIndex = 0;
  let effectIndex = 0;
  let timerId = 0;
  let now = 0;
  let dirty = false;
  let current = null;
  let props = { prompts, paused: options.paused ?? false };
  const exports = {};
  vm.runInNewContext(source, {
    exports,
    document,
    window: {
      matchMedia(query) {
        assert.equal(query, "(prefers-reduced-motion: reduce)");
        return media;
      },
      setTimeout(callback, delay) {
        timers.set(++timerId, { callback, due: now + delay });
        return timerId;
      },
      clearTimeout(id) { timers.delete(id); }
    },
    require(id) {
      assert.equal(id, "react");
      return {
        useState(initial) {
          const index = stateIndex++;
          if (!(index in state)) state[index] = typeof initial === "function" ? initial() : initial;
          return [state[index], (update) => {
            const next = typeof update === "function" ? update(state[index]) : update;
            if (!Object.is(state[index], next)) {
              state[index] = next;
              dirty = true;
            }
          }];
        },
        useEffect(callback, dependencies) {
          const index = effectIndex++;
          const previous = effects[index];
          if (!previous || dependencies.some((value, position) => !Object.is(value, previous.dependencies[position]))) {
            effects[index] = { callback, dependencies, cleanup: previous?.cleanup, pending: true };
          }
        }
      };
    }
  }, { filename });

  function render() {
    let attempts = 0;
    do {
      assert.ok(++attempts < 20, "hook must settle after state/effect updates");
      dirty = false;
      stateIndex = 0;
      effectIndex = 0;
      current = exports.useAssistantPromptRotation(props.prompts, props.paused);
      for (const effect of effects) {
        if (!effect.pending) continue;
        effect.pending = false;
        effect.cleanup?.();
        effect.cleanup = effect.callback();
      }
    } while (dirty);
    return current;
  }
  render();
  return {
    get current() { return current; },
    get label() { return current?.label ?? null; },
    get timerCount() { return timers.size; },
    get listenerCount() { return document.listenerCount() + media.listenerCount(); },
    update(values) { props = { ...props, ...values }; render(); },
    advance(milliseconds) {
      const end = now + milliseconds;
      while (true) {
        const next = [...timers].sort((left, right) => left[1].due - right[1].due)[0];
        if (!next || next[1].due > end) break;
        timers.delete(next[0]);
        now = next[1].due;
        next[1].callback();
        render();
      }
      now = end;
    },
    visibility(hidden) { document.hidden = hidden; document.dispatch("visibilitychange"); render(); },
    motion(reduced) { media.matches = reduced; media.dispatch("change"); render(); },
    unmount() { effects.forEach((effect) => effect.cleanup?.()); }
  };
}

test("one suggestion rotates every five seconds and wraps", (t) => {
  const harness = rotationHarness([prompt("a"), prompt("b"), prompt("c")]);
  t.after(() => harness.unmount());
  assert.equal(harness.label, "a");
  harness.advance(4_999);
  assert.equal(harness.label, "a");
  harness.advance(1);
  assert.equal(harness.label, "b");
  harness.advance(5_000);
  assert.equal(harness.label, "c");
  harness.advance(5_000);
  assert.equal(harness.label, "a");
  assert.equal(harness.timerCount, 1);
});

test("refreshed prompt objects preserve timing and keep the visible prompt by ID", (t) => {
  const harness = rotationHarness([prompt("a"), prompt("b"), prompt("c")]);
  t.after(() => harness.unmount());
  harness.advance(4_000);
  harness.update({ prompts: [prompt("a", "latest a"), prompt("b", "latest b"), prompt("c", "latest c")] });
  assert.equal(harness.label, "latest a");
  harness.advance(1_000);
  assert.equal(harness.label, "latest b");
  harness.advance(4_000);
  const replacement = [prompt("d"), prompt("e")];
  harness.update({ prompts: replacement });
  assert.equal(harness.label, "d");
  harness.advance(4_999);
  assert.equal(harness.label, "d");
  harness.advance(1);
  assert.equal(harness.label, "e");
  harness.update({ prompts: [replacement[1], replacement[0]] });
  assert.equal(harness.label, "e", "reordering keeps the visible prompt by ID");
});

test("a parent pause cancels timers until resumed", (t) => {
  const harness = rotationHarness([prompt("a"), prompt("b"), prompt("c")]);
  t.after(() => harness.unmount());
  harness.update({ paused: true });
  assert.equal(harness.timerCount, 0);
  harness.advance(10_000);
  assert.equal(harness.label, "a");
  harness.update({ paused: false });
  harness.advance(5_000);
  assert.equal(harness.label, "b");
});

test("hidden pages and reduced motion suppress automatic rotation", (t) => {
  const harness = rotationHarness([prompt("a"), prompt("b"), prompt("c")], { hidden: true });
  t.after(() => harness.unmount());
  assert.equal(harness.timerCount, 0);
  harness.visibility(false);
  harness.advance(5_000);
  assert.equal(harness.label, "b");
  harness.motion(true);
  assert.equal(harness.timerCount, 0);
  harness.advance(10_000);
  assert.equal(harness.label, "b");
  harness.motion(false);
  harness.advance(5_000);
  assert.equal(harness.label, "c");
  harness.visibility(true);
  harness.advance(10_000);
  assert.equal(harness.label, "c");
});

test("initial reduced motion, empty/single lists and unmount leave no unnecessary timer or listener", () => {
  const reduced = rotationHarness([prompt("a"), prompt("b")], { reducedMotion: true });
  assert.equal(reduced.timerCount, 0);
  reduced.unmount();
  assert.equal(reduced.listenerCount, 0);

  const harness = rotationHarness([]);
  assert.equal(harness.current, null);
  assert.equal(harness.timerCount, 0);
  harness.update({ prompts: [prompt("a")] });
  assert.equal(harness.timerCount, 0);
  assert.equal(harness.label, "a");
  harness.update({ prompts: [prompt("a"), prompt("b")] });
  assert.equal(harness.timerCount, 1);
  assert.equal(harness.listenerCount, 2);
  harness.unmount();
  assert.equal(harness.timerCount, 0);
  assert.equal(harness.listenerCount, 0);
});
