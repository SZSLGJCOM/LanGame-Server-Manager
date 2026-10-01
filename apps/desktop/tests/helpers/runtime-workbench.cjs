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

// Exercise the component's real effects and functional state updates without a
// native window. This fixture does not model React rendering or DOM resources.
function mountWorkbench(registration, options = {}) {
  const effects = [];
  const writes = [];
  const states = [];
  const listeners = new Map();
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
      useEffect: (effect) => effects.push(effect),
      useLayoutEffect: (effect) => effects.push(effect),
      useMemo: (read) => read(),
      useRef: (current) => ({ current }),
      useState: (initial) => {
        const index = states.length;
        states.push(index === 0 && options.commandDraft !== undefined ? options.commandDraft
          : typeof initial === "function" ? initial() : initial);
        return [states[index], (update) => {
          states[index] = typeof update === "function" ? update(states[index]) : update;
          writes.push(states[index]);
        }];
      }
    };
    if (id === "../../i18n") return { useI18n: () => ({ locale: "en", t: (key, _params, fallback) => fallback || key }),
      selectLocaleText: (_locale, _zh, en) => en };
    if (id === "@tauri-apps/api/core") return { isTauri: () => true };
    if (id === "@tauri-apps/api/event") return { listen: (name, callback) => {
      assert.ok(["runtime-log-stream", "runtime-service-events-reset"].includes(name), `Unexpected native event: ${name}`);
      assert.equal(listeners.has(name), false, `Duplicate native event listener: ${name}`);
      listeners.set(name, callback);
      return name === "runtime-log-stream" ? registration.promise : recoveryRegistration.promise;
    } };
    // The native publisher writes to the retained file before emitting. Model
    // that external boundary; the component still owns refresh and lifetime.
    if (id === "../../api") return { readInstanceLogDocument: options.readLogDocument ??
      (async () => ({ source_path: logPath, lines: retainedLines.slice(-400),
        total_lines: retainedLines.length, truncated: retainedLines.length > 400, read_error: null })) };
    if (id === "../../app-state") return { describeError: String };
    if (id === "../../components/ShellIcon") return { ShellIcon: () => null };
    return originalRequire(id);
  };
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  const tree = loaded.exports.RuntimeSurfaceWorkbench({
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
  });
  const cleanups = effects.map((effect) => effect());
  let disposed = false;
  let emittedByteOffset = 0;
  return {
    tree,
    writes,
    registrations: [...listeners.keys()],
    // This hook-only fixture observes the selected live document, not the
    // separately retained IPC response. Browser fixtures verify its rendering.
    snapshot: () => states[6] ?? null,
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
      for (const cleanup of cleanups) cleanup?.();
    }
  };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));
module.exports = { deferred, mountWorkbench, settle };
