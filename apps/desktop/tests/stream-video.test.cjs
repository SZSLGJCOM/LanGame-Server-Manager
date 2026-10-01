const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const media = require("../src/official-media-sources.ts");
const directCache = { createMediaCacheResolver: () => Object.assign((source) => ({ then(callback) { callback(source); } }), { disable() {} }) };
require.cache[path.resolve(__dirname, "../src/media-cache.ts")] = { exports: directCache };
const { createOfficialHlsLoader } = require("../src/official-hls-loader.ts");

function mountVideo({ streamUrl = "https://media.invalid/trailer.m3u8", loadHls, play, autoPlay = true, preload = "auto", locale = "en-US" } = {}) {
  const video = new EventTarget();
  const warnings = [];
  const instances = [];
  let effect;
  let errors = 0;
  let reportedFailures = 0;
  let plays = 0;
  let pauses = 0;
  let loads = 0;
  let refIndex = 0;
  const refs = [];
  const timers = new Map();
  let timerId = 0;
  const clock = {
    setTimeout(callback) { timers.set(++timerId, callback); return timerId; },
    clearTimeout(id) { timers.delete(id); }
  };
  Object.assign(video, {
    readyState: 0,
    muted: true,
    paused: true,
    currentTime: 0,
    duration: 120,
    preload,
    play: () => { plays += 1; video.paused = false; return play ? play() : Promise.resolve(); },
    pause: () => { pauses += 1; video.paused = true; },
    removeAttribute: (name) => { delete video[name]; },
    load: () => { loads += 1; }
  });
  video.addEventListener("error", () => { errors += 1; });
  class Hls {
    static Events = { MANIFEST_PARSED: "manifest", ERROR: "error" };
    static DefaultConfig = { loader: class {} };
    static isSupported = () => true;
    handlers = new Map();
    stops = 0;
    destroys = 0;
    constructor(config) { this.config = config; instances.push(this); }
    loadSource(url) { this.url = url; }
    attachMedia(media) { this.media = media; }
    on(event, callback) { this.handlers.set(event, callback); }
    stopLoad() { this.stops += 1; }
    destroy() { this.destroys += 1; }
  }
  const runtimeFilename = path.join(__dirname, "../src/stream-video-player.ts");
  const runtime = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(runtimeFilename, "utf8"), runtimeFilename), {
    exports: runtime, URL, window: clock,
    console: { warn: (...args) => warnings.push(args) },
    require: (id) => {
      if (id === "./official-media-sources") return media;
      if (id === "./official-hls-loader") return { createOfficialHlsLoader };
      if (id === "./locale-preference") return { readPreferredLocale: () => locale };
      if (id === "./media-cache") return directCache;
      if (id === "hls.js") return loadHls ? loadHls() : { __esModule: true, default: Hls };
      throw new Error(`Unexpected runtime dependency: ${id}`);
    }
  }, { filename: runtimeFilename });
  const filename = path.join(__dirname, "../src/components/StreamVideo.tsx");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    Event,
    console: { warn: (...args) => warnings.push(args) },
    require: (id) => {
      if (id === "react") return { useRef: (initial) => { const index = refIndex++; return refs[index] ??= { current: index === 0 ? video : initial }; }, useEffect: (callback) => { effect = callback; } };
      if (id === "react/jsx-runtime") return { jsx: (type, props) => ({ type, props }) };
      if (id === "../stream-video-player") return runtime;
      if (id === "../i18n") return { useI18n: () => ({ locale }) };
      throw new Error(`Unexpected dependency: ${id}`);
    }
  }, { filename });
  const render = () => exports.StreamVideo({ streamUrl, autoPlay, preload, muted: true, loop: true,
    onError: () => { reportedFailures += 1; } });
  let element = render();
  video.addEventListener("error", () => element.props.onError({ type: "error" }));
  let activeCleanup = effect();
  const cleanup = () => activeCleanup();
  return { video, element, instances, cleanup, warnings, timers, errors: () => errors, plays: () => plays,
    pauses: () => pauses, loads: () => loads,
    failures: () => reportedFailures,
    switchLocale(next) { activeCleanup(); locale = next; refIndex = 0; element = render(); activeCleanup = effect(); },
    timeout() { const callbacks = [...timers.values()]; timers.clear(); for (const callback of callbacks) callback(); } };
}

const flush = () => new Promise((resolve) => setImmediate(resolve));

test("native video follows language changes without losing the same video's playhead", () => {
  media.forgetMediaSourcePreferences();
  const source = "https://video.akamai.steamstatic.com/store_trailers/locale/movie.mp4";
  const preview = mountVideo({ streamUrl: source, locale: "zh-CN" });
  assert.equal(new URL(preview.video.src).hostname, "video.cdn.steamchina.queniuam.com");
  preview.video.currentTime = 37;
  preview.video.paused = false;
  preview.switchLocale("en-US");
  assert.equal(preview.video.src, source);
  preview.video.currentTime = 0;
  preview.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(preview.video.currentTime, 37);
  preview.switchLocale("zh-CN");
  assert.equal(new URL(preview.video.src).hostname, "video.cdn.steamchina.queniuam.com");
  preview.cleanup();
});

test("a selected HLS preview starts on its manifest and retains looping", async () => {
  const preview = mountVideo();
  await flush();
  const hls = preview.instances[0];
  assert.ok(Object.is(hls.media, preview.video));
  assert.equal(preview.element.props.loop, true);
  hls.handlers.get("manifest")();
  await flush();
  assert.equal(preview.plays(), 1);
  preview.cleanup();
  assert.equal(hls.stops, 1);
  assert.equal(hls.destroys, 1);
});

test("a rejected lazy HLS module reports failure so the cover can recover", async () => {
  const failure = new TypeError("Failed to fetch dynamically imported module");
  const preview = mountVideo({ loadHls: () => { throw failure; } });
  await flush();
  assert.equal(preview.errors(), 1);
  assert.equal(preview.warnings.length, 1);
  preview.cleanup();
});

test("switching cards ignores an obsolete HLS import failure", async () => {
  const preview = mountVideo({ loadHls: () => { throw new TypeError("stale module"); } });
  preview.cleanup();
  await flush();
  assert.equal(preview.errors(), 0);
});

test("switching cards before HLS loads never attaches an obsolete player", async () => {
  const preview = mountVideo();
  preview.cleanup();
  await flush();
  assert.equal(preview.instances.length, 0);
  assert.equal(preview.plays(), 0);
});

test("a rejected muted autoplay reports failure without retrying the same policy", async () => {
  const preview = mountVideo({
    streamUrl: "https://media.invalid/trailer.mp4",
    play: () => Promise.reject(new Error("playback unavailable"))
  });
  preview.video.dispatchEvent(new Event("canplay"));
  await flush();
  assert.equal(preview.plays(), 1);
  assert.equal(preview.errors(), 1);
  assert.equal(preview.warnings.length, 1);
  preview.cleanup();
});

test("switching cards does not retry an obsolete autoplay promise", async () => {
  let rejectPlay;
  const preview = mountVideo({
    streamUrl: "https://media.invalid/trailer.mp4",
    play: () => new Promise((_, reject) => { rejectPlay = reject; })
  });
  preview.video.dispatchEvent(new Event("canplay"));
  preview.cleanup();
  rejectPlay(new Error("interrupted playback"));
  await flush();
  assert.equal(preview.plays(), 1);
  assert.equal(preview.errors(), 0);
});

test("an unmounted native video cannot play from a late canplay event", async () => {
  const preview = mountVideo({ streamUrl: "https://media.invalid/trailer.mp4" });
  preview.cleanup();
  preview.video.dispatchEvent(new Event("canplay"));
  await flush();
  assert.equal(preview.plays(), 0);
});

test("a fatal stream failure releases the HLS instance exactly once", async () => {
  const preview = mountVideo();
  await flush();
  const hls = preview.instances[0];
  hls.handlers.get("error")("error", { fatal: true });
  preview.cleanup();
  assert.equal(preview.errors(), 1);
  assert.equal(hls.stops, 1);
  assert.equal(hls.destroys, 1);
});

test("native media fails over only to the same video, retains playback position and reports only exhaustion", () => {
  media.forgetMediaSourcePreferences();
  const streamUrl = "https://video.akamai.steamstatic.com/store_trailers/123/trailer.mp4?t=3";
  const preview = mountVideo({ streamUrl });
  assert.equal(preview.video.src, streamUrl);
  preview.video.currentTime = 42;
  preview.video.paused = false;
  preview.video.dispatchEvent(new Event("error"));
  assert.equal(preview.failures(), 0);
  assert.equal(preview.video.src, streamUrl.replace("video.akamai", "video.fastly"));
  preview.video.currentTime = 0;
  preview.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(preview.video.currentTime, 42);
  assert.equal(preview.video.muted, true);
  preview.video.dispatchEvent(new Event("error"));
  assert.equal(preview.video.src, streamUrl.replace("video.akamai.steamstatic.com", "video.cdn.steamchina.queniuam.com"));
  preview.video.dispatchEvent(new Event("error"));
  preview.video.dispatchEvent(new Event("error"));
  assert.equal(preview.failures(), 1);
  preview.cleanup();
  assert.equal(preview.timers.size, 0);
  media.forgetMediaSourcePreferences();
});

test("slow native media advances with finite deadlines and unmount clears the active deadline", () => {
  const streamUrl = "https://video.akamai.steamstatic.com/store_trailers/999/trailer.webm";
  const preview = mountVideo({ streamUrl });
  preview.timeout();
  assert.equal(preview.video.src, streamUrl.replace("video.akamai", "video.fastly"));
  preview.cleanup();
  preview.timeout();
  assert.equal(preview.failures(), 0);
  assert.equal(preview.timers.size, 0);
});

test("consecutive failed native sources retain the original playhead until an alternative is ready", () => {
  media.forgetMediaSourcePreferences();
  const preview = mountVideo({ streamUrl: "https://video.akamai.steamstatic.com/store_trailers/888/movie.mp4" });
  preview.video.currentTime = 25;
  preview.video.paused = false;
  preview.video.dispatchEvent(new Event("error"));
  preview.video.currentTime = 0;
  preview.video.paused = true;
  preview.video.dispatchEvent(new Event("error"));
  preview.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(preview.video.currentTime, 25);
  preview.video.currentTime = 26;
  preview.video.dispatchEvent(new Event("canplay"));
  assert.equal(preview.video.currentTime, 26, "canplay must not seek to the saved position a second time");
  preview.cleanup();
  media.forgetMediaSourcePreferences();
});

test("native autoplay remains bounded after metadata until playable data arrives", () => {
  media.forgetMediaSourcePreferences();
  const source = "https://video.akamai.steamstatic.com/store_trailers/777/movie.mp4";
  const preview = mountVideo({ streamUrl: source });
  preview.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(preview.timers.size, 1);
  preview.timeout();
  assert.equal(preview.video.src, source.replace("video.akamai", "video.fastly"));
  preview.video.dispatchEvent(new Event("canplay"));
  assert.equal(preview.timers.size, 0);
  preview.cleanup();
  media.forgetMediaSourcePreferences();
});

test("metadata-only paused videos wait for the user, then bound play and buffering without extending repeated waiting events", () => {
  media.forgetMediaSourcePreferences();
  const source = "https://video.akamai.steamstatic.com/store_trailers/776/movie.mp4";
  const preview = mountVideo({ streamUrl: source, autoPlay: false, preload: "metadata" });
  preview.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(preview.timers.size, 0);
  preview.timeout();
  assert.equal(preview.video.src, source);
  preview.video.paused = false;
  preview.video.dispatchEvent(new Event("play"));
  const timer = [...preview.timers.keys()][0];
  preview.video.dispatchEvent(new Event("waiting"));
  preview.video.dispatchEvent(new Event("waiting"));
  assert.deepEqual([...preview.timers.keys()], [timer]);
  preview.timeout();
  assert.equal(preview.video.src, source.replace("video.akamai", "video.fastly"));
  preview.video.dispatchEvent(new Event("canplay"));
  assert.equal(preview.timers.size, 0);
  preview.video.dispatchEvent(new Event("waiting"));
  assert.equal(preview.timers.size, 1);
  preview.video.paused = true;
  preview.video.dispatchEvent(new Event("pause"));
  assert.equal(preview.timers.size, 0);
  preview.cleanup();
  media.forgetMediaSourcePreferences();
});

test("native source exhaustion stops the last request and late events or repeated disposal cannot restart playback", () => {
  media.forgetMediaSourcePreferences();
  const preview = mountVideo({ streamUrl: "https://video.akamai.steamstatic.com/store_trailers/775/movie.mp4" });
  preview.timeout(); preview.timeout(); preview.timeout();
  assert.equal(preview.failures(), 1);
  assert.equal(preview.video.src, undefined, "the exhausted source is removed before reporting failure");
  assert.equal(preview.video.paused, true);
  assert.equal(preview.pauses(), 3);
  assert.equal(preview.loads(), 6, "each prior candidate is released before resolving its replacement, including terminal failure");
  assert.equal(preview.timers.size, 0);
  for (const event of ["error", "loadedmetadata", "canplay", "play", "waiting"]) preview.video.dispatchEvent(new Event(event));
  preview.timeout();
  preview.cleanup(); preview.cleanup();
  assert.equal(preview.failures(), 1);
  assert.equal(preview.plays(), 0);
  assert.equal(preview.pauses(), 3);
  assert.equal(preview.loads(), 6);
  assert.equal(preview.video.src, undefined);
  assert.equal(preview.timers.size, 0);
});
