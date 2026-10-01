const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);

function loadSource(relative, overrides = {}, globals = {}) {
  const filename = path.resolve(__dirname, "../src", relative);
  const exports = {};
  const nativeRequire = Module.createRequire(filename);
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, URL, Date, Headers, Event, console, setTimeout, clearTimeout, ...globals,
    require: (id) => overrides[id] ?? nativeRequire(id)
  }, { filename });
  return exports;
}

const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() { let resolve; let reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function clock() {
  const pending = new Map();
  const deadlines = new Map();
  let sequence = 0;
  let now = 0;
  return { pending, setTimeout(fn, delay = 0) { pending.set(++sequence, fn); deadlines.set(sequence, now + delay); return sequence; },
    clearTimeout(id) { pending.delete(id); deadlines.delete(id); },
    tick() { const callbacks = [...pending.values()]; pending.clear(); deadlines.clear(); for (const callback of callbacks) callback(); },
    advance(milliseconds) {
      now += milliseconds;
      const due = [...pending].filter(([id]) => deadlines.get(id) <= now);
      for (const [id, callback] of due) { pending.delete(id); deadlines.delete(id); callback(); }
    } };
}

function cacheEnvironment({ tauri = false, register = async () => "/__langame/media/opaque-fixture" } = {}) {
  const calls = [];
  const timers = clock();
  const cache = loadSource("media-cache.ts", {
    "@tauri-apps/api/core": { isTauri: () => tauri },
    "./api": { registerMediaCacheSource(...args) { calls.push(args); return register(...args); } }
  }, timers);
  return { cache, calls, timers };
}

class ImageFixture extends EventTarget {
  constructor(source) { super(); this.attributes = source ? { src: source } : {}; this.requests = []; this.complete = false; this.naturalWidth = 0; }
  getAttribute(name) { return this.attributes[name] ?? null; }
  removeAttribute(name) { delete this.attributes[name]; }
  set src(source) { this.attributes.src = source; this.requests.push(source); }
  get src() { return this.attributes.src; }
  emit(event) { this[`on${event}`]?.(); this.dispatchEvent(new Event(event)); }
}

function nativeVideo(cache) {
  const timers = clock();
  const video = new EventTarget();
  const requests = [];
  let source;
  let failures = 0;
  Object.defineProperty(video, "src", { get: () => source, set(value) { source = value; requests.push(value); } });
  Object.assign(video, { readyState: 0, muted: true, paused: true, currentTime: 0, duration: 120, preload: "auto",
    play: () => { video.paused = false; return Promise.resolve(); }, pause: () => { video.paused = true; },
    removeAttribute(name) { if (name === "src") source = undefined; }, load: () => { video.currentTime = 0; } });
  const runtime = loadSource("stream-video-player.ts", { "./media-cache": cache, "./official-hls-loader": {} }, { window: timers });
  return { video, requests, timers, failures: () => failures,
    attach(streamUrl, locale = "en-US") { return runtime.attachStreamVideo(video, { streamUrl, locale, autoPlay: true, onUnavailable() { failures++; } }); } };
}

function imageHook(cache) {
  const slots = [];
  const timers = clock();
  let cursor = 0;
  let effects = [];
  let locale = "en-US";
  const same = (left, right) => left?.length === right?.length && left.every((item, index) => Object.is(item, right[index]));
  const react = {
    useRef(value) { const index = cursor++; return slots[index] ??= { current: value }; },
    useState(value) { const index = cursor++; slots[index] ??= { value }; return [slots[index].value, (next) => { slots[index].value = typeof next === "function" ? next(slots[index].value) : next; }]; },
    useMemo(factory, deps) { const index = cursor++; if (!slots[index] || !same(slots[index].deps, deps)) slots[index] = { deps, value: factory() }; return slots[index].value; },
    useEffect(callback, deps) { const index = cursor++; if (!slots[index] || !same(slots[index].deps, deps)) effects.push(() => { slots[index]?.cleanup?.(); slots[index] = { deps, callback, cleanup: callback() }; }); }
  };
  const { useMediaSource } = loadSource("components/useMediaSource.ts", { react, "../media-cache": cache, "../i18n": { useI18n: () => ({ locale }) } }, { window: timers });
  return { timers, setLocale(next) { locale = next; },
    render(sources) { cursor = 0; const result = useMediaSource(sources); result.ref.current = { loading: "eager" }; const pending = effects; effects = []; for (const run of pending) run(); return result; },
    replay() { for (const slot of slots) if (slot?.callback) { slot.cleanup?.(); slot.cleanup = slot.callback(); } },
    cleanup() { for (const slot of slots) slot?.cleanup?.(); } };
}

module.exports = { loadSource, flush, deferred, clock, cacheEnvironment, ImageFixture, nativeVideo, imageHook };
