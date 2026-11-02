const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { transpileTypeScript } = require("../../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (loaded, filename) => {
    loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((onResolve, onReject) => { resolve = onResolve; reject = onReject; });
  return { promise, resolve, reject };
}

// Exercise component state transitions and dependency-owned effects without a
// native window. Real React rendering and DOM resources stay in browser tests.
function mountWorkbench(registration, options = {}) {
  const writes = [];
  const states = [];
  const slots = [];
  const listeners = new Map();
  const registrations = [];
  let cursor = 0;
  let pendingEffects = [];
  let disposed = false;
  let scheduled = false;
  let tree;
  const sameDependencies = (previous, next) => previous && next && previous.length === next.length
    && next.every((value, index) => Object.is(value, previous[index]));
  function scheduleRender() {
    if (disposed || scheduled) return;
    scheduled = true;
    queueMicrotask(() => { scheduled = false; if (!disposed) render(); });
  }
  function effectHook(effect, dependencies) {
    const index = cursor++;
    const previous = slots[index];
    if (previous && sameDependencies(previous.dependencies, dependencies)) return;
    const slot = { dependencies, effect, cleanup: previous?.cleanup };
    slots[index] = slot;
    pendingEffects.push(slot);
  }
  const translate = (key, _params, fallback) => fallback || key;
  const recoveryRegistration = options.recoveryRegistration ?? { promise: Promise.resolve(() => {}) };
  const instanceId = options.instanceId ?? "fixture";
  const logPath = options.logPath ?? "fixture/run-1-main.log";
  const retainedLines = [...(options.runtime?.log_tail?.lines ?? [])];
  const filename = path.join(__dirname, "../../src/views/servers/RuntimeSurfaceWorkbench.tsx");
  const originalRequire = Module.createRequire(filename);
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  loaded.require = (id) => {
    if (id === "react") return {
      useEffect: effectHook,
      useLayoutEffect: effectHook,
      useMemo: (read, dependencies) => {
        const index = cursor++;
        if (!slots[index] || !sameDependencies(slots[index].dependencies, dependencies)) {
          slots[index] = { value: read(), dependencies };
        }
        return slots[index].value;
      },
      useId: () => "runtime-tooltip",
      useRef: (current) => {
        const index = cursor++;
        slots[index] ??= { value: { current } };
        return slots[index].value;
      },
      useState: (initial) => {
        const index = cursor++;
        if (!slots[index]) {
          const stateIndex = states.length;
          states.push(stateIndex === 0 && options.commandDraft !== undefined ? options.commandDraft
            : typeof initial === "function" ? initial() : initial);
          slots[index] = { stateIndex, update(next) {
            const previous = states[stateIndex];
            states[stateIndex] = typeof next === "function" ? next(previous) : next;
            writes.push(states[stateIndex]);
            if (!Object.is(previous, states[stateIndex])) scheduleRender();
          } };
        }
        return [states[slots[index].stateIndex], slots[index].update];
      }
    };
    if (id === "../../i18n") return { useI18n: () => ({ locale: "en", t: translate }),
      selectLocaleText: (_locale, _zh, en) => en };
    if (id === "@tauri-apps/api/core") return { isTauri: () => true };
    if (id === "@tauri-apps/api/event") return { listen: (name, callback) => {
      assert.ok(["runtime-log-stream", "runtime-service-events-reset"].includes(name), `Unexpected native event: ${name}`);
      const entry = { name, callback, active: false, released: false };
      registrations.push(entry);
      listeners.set(name, callback);
      const source = name === "runtime-log-stream" ? registration : recoveryRegistration;
      const request = typeof source === "function" ? source() : source.promise;
      return request.then(release => {
        entry.active = true;
        return () => {
          assert.equal(entry.released, false, "A native registration must be released exactly once");
          entry.active = false;
          entry.released = true;
          release();
        };
      });
    } };
    // The native publisher writes to the retained file before emitting. Model
    // that external boundary; the component still owns refresh and lifetime.
    if (id === "../../api") return { readInstanceLogDocument: options.readLogDocument ??
      (async () => ({ source_path: logPath, lines: retainedLines.slice(-400),
        total_lines: retainedLines.length, truncated: retainedLines.length > 400, read_error: null })) };
    if (id === "../../app-state") return { describeError: String };
    if (id === "../../components/ShellIcon") return { ShellIcon: () => null };
    if (id === "./SteamWorkshopAccountConsole") return { SteamWorkshopAccountConsole: () => null };
    return originalRequire(id);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const props = {
    moduleDetails: options.moduleDetails === undefined ? {
      summary: { id: "dontstarve" }, runtime: { shutdown: { commands: [{ transport: "stdin" }] } }
    } : options.moduleDetails,
    details: options.details ?? { summary: { id: instanceId, module_id: "dontstarve", status: "stopped", active_process_count: 0 }, active_run: null },
    runtime: options.runtime ?? { recent_runs: [], log_tail: { source_path: logPath, lines: [], total_lines: 0, truncated: false, read_error: null }, health: { status: "stopped", summary: "Stopped", reason: null }, diagnostics: [] },
    runtimeWindows: null, startupPending: options.startupPending ?? false, launchHostSurface: "managed_terminal",
    startupBoundary: options.startupBoundary,
    archive: options.archive,
    onSendRuntimeCommand: options.onSendRuntimeCommand ?? (async () => null), onSuppressRuntimeWindows: async () => {},
    onRetryReads: options.onRetryReads
  };
  function render() {
    cursor = 0;
    pendingEffects = [];
    tree = loaded.exports.RuntimeSurfaceWorkbench(props);
    while (tree) {
      if (typeof tree.type === "function") tree = tree.type(tree.props);
      else if (tree.type === Symbol.for("react.fragment")) {
        const children = Array.isArray(tree.props.children) ? tree.props.children : [tree.props.children];
        tree = children.find((child) => typeof child?.type === "function");
      } else break;
    }
    const effects = pendingEffects;
    for (const slot of effects) slot.cleanup?.();
    for (const slot of effects) slot.cleanup = slot.effect();
  }
  render();
  let emittedByteOffset = 0;
  return {
    get tree() { return tree; },
    writes,
    get registrations() { return registrations.map(entry => entry.name); },
    get activeRegistrations() { return registrations.filter(entry => entry.active); },
    // This hook-only fixture observes the selected live document, not the
    // separately retained IPC response. Browser fixtures verify its rendering.
    snapshot: () => states[6] ?? null,
    diagnostic: () => states[10] ?? null,
    receive: (payload) => {
      const event = { instance_id: instanceId, log_path: logPath, lines: [],
        byte_offset: ++emittedByteOffset, emitted_at_unix_ms: 1, ...payload };
      if (event.instance_id === instanceId && event.log_path === logPath) retainedLines.push(...event.lines);
      listeners.get("runtime-log-stream")({ payload: event });
    },
    reset: () => listeners.get("runtime-service-events-reset")({ payload: {} }),
    dispose: () => {
      if (disposed) return;
      disposed = true;
      for (const slot of slots) slot?.cleanup?.();
    }
  };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));
module.exports = { deferred, mountWorkbench, settle };
