const assert = require("node:assert/strict");
const test = require("node:test");
const { loadSource, flush, clock } = require("./media-cache-test-support.cjs");

const webm = "https://publisher.invalid/story.webm";
const mp4 = "https://publisher.invalid/story.mp4";

function richVideo({ supported = () => "probably", formats = [webm, mp4], hls } = {}) {
  const timers = clock();
  const requests = [];
  const warnings = [];
  const children = formats.map((src) => ({ src, type: src.endsWith(".webm") ? "video/webm; codecs=vp9" : "video/mp4",
    getAttribute(name) { return name === "src" ? this.src : null; }, removeAttribute() { this.src = null; } }));
  const video = new EventTarget();
  let source;
  let plays = 0;
  Object.defineProperty(video, "src", { get: () => source, set(value) { source = value; requests.push(value); } });
  Object.assign(video, { readyState: 0, currentSrc: "", muted: false, defaultMuted: true, autoplay: true,
    paused: true, currentTime: 0, duration: 120, preload: "auto", canPlayType: supported,
    getAttribute: () => null, querySelectorAll: () => children.filter((child) => child.src),
    play() { plays++; this.paused = false; return this.muted ? Promise.resolve() : Promise.reject(new Error("unmuted autoplay denied")); },
    pause() { this.paused = true; }, removeAttribute() { source = undefined; }, load() { this.currentTime = 0; }
  });
  const cache = { createMediaCacheResolver: () => Object.assign(async (source) => source, { disable() {} }) };
  const player = loadSource("stream-video-player.ts", { "./media-cache": cache,
    "./official-hls-loader": { createOfficialHlsLoader: () => class {} },
    ...(hls ? { "hls.js": { __esModule: true, default: hls } } : {}) }, {
    window: timers, console: { warn: (...args) => warnings.push(args) }
  });
  const { attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
    "./media-cache": cache, "./stream-video-player": player
  }, timers);
  const container = { querySelectorAll: (selector) => selector === "video" ? [video] : [] };
  return { video, requests, timers, warnings, plays: () => plays,
    attach: (locale = "en-US") => attachHtmlMediaFallbacks(container, locale) };
}

test("Steam-style WebM failure falls through to the declared MP4 and retains playback state", async () => {
  const fixture = richVideo();
  const cleanup = fixture.attach();
  await flush();
  assert.equal(fixture.video.src, webm);
  fixture.video.currentTime = 7;
  fixture.video.paused = false;
  fixture.video.dispatchEvent(new Event("error")); await flush();
  assert.equal(fixture.video.src, mp4, "the remaining source format must survive inert insertion and primary failure");
  fixture.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(fixture.video.currentTime, 7);
  fixture.video.dispatchEvent(new Event("canplay")); await flush();
  assert.equal(fixture.video.paused, false);
  assert.deepEqual(fixture.warnings, []);
  cleanup();
  assert.equal(fixture.timers.pending.size, 0);
});

test("default-muted HTML animations are muted before autoplay, while user unmuting survives effect replay", async () => {
  const fixture = richVideo();
  let cleanup = fixture.attach(); await flush();
  assert.equal(fixture.video.muted, true, "the HTML muted attribute must configure playback, not only defaultMuted");
  fixture.video.dispatchEvent(new Event("canplay")); await flush();
  assert.equal(fixture.plays(), 1);
  assert.deepEqual(fixture.warnings, []);
  fixture.video.muted = false;
  cleanup();
  cleanup = fixture.attach("zh-CN"); await flush();
  assert.equal(fixture.video.muted, false, "locale/effect replay must preserve the user's explicit mute choice");
  cleanup();
});

test("decode errors skip equivalent CDNs and try the next declared format", async () => {
  const primary = "https://video.akamai.steamstatic.com/store_trailers/1/story.webm";
  const alternate = primary.replace(".webm", ".mp4");
  const fixture = richVideo({ formats: [primary, alternate] });
  const cleanup = fixture.attach(); await flush();
  fixture.video.error = { code: 3 };
  fixture.video.dispatchEvent(new Event("error")); await flush();
  assert.deepEqual(fixture.requests, [primary, alternate]);
  cleanup();
});

test("source-not-supported errors retain CDN recovery because HTTP failures also report code 4", async () => {
  const primary = "https://video.akamai.steamstatic.com/store_trailers/2/story.webm";
  const fixture = richVideo({ formats: [primary, primary.replace(".webm", ".mp4")] });
  const cleanup = fixture.attach(); await flush();
  fixture.video.error = { code: 4 };
  for (let index = 0; index < 3; index++) { fixture.video.dispatchEvent(new Event("error")); await flush(); }
  assert.deepEqual(fixture.requests, [primary,
    primary.replace("video.akamai", "video.fastly"),
    primary.replace("video.akamai.steamstatic.com", "video.cdn.steamchina.queniuam.com"),
    primary.replace(".webm", ".mp4")]);
  cleanup();
});

test("fatal HLS failure can recover to a declared native format and obsolete HLS events cannot restart it", async () => {
  const instances = [];
  class Hls {
    static isSupported = () => true;
    static Events = { MANIFEST_PARSED: "manifest", ERROR: "error" };
    static DefaultConfig = { loader: class {} };
    handlers = new Map();
    destroyed = 0;
    constructor() { instances.push(this); }
    on(event, callback) { this.handlers.set(event, callback); }
    loadSource() {}
    attachMedia() {}
    stopLoad() {}
    destroy() { this.destroyed++; }
  }
  const fixture = richVideo({ formats: ["https://publisher.invalid/story.m3u8", mp4], hls: Hls });
  const cleanup = fixture.attach(); await flush();
  const hls = instances[0];
  hls.handlers.get("error")("error", { fatal: true }); await flush();
  assert.equal(fixture.video.src, mp4);
  assert.equal(hls.destroyed, 1);
  hls.handlers.get("manifest")();
  hls.handlers.get("error")("error", { fatal: true }); await flush();
  assert.deepEqual(fixture.requests, [mp4]);
  assert.equal(fixture.plays(), 0);
  cleanup();
  assert.equal(hls.destroyed, 1);
  assert.equal(fixture.timers.pending.size, 0);
});

test("unsupported declared codecs are skipped and all failed formats stop without reviving child sources", async () => {
  const unsupported = richVideo({ supported: (type) => type.includes("webm") ? "" : "probably" });
  const cancelUnsupported = unsupported.attach(); await flush();
  assert.deepEqual(unsupported.requests, [mp4]);
  cancelUnsupported();

  const fixture = richVideo();
  const cleanup = fixture.attach(); await flush();
  fixture.timers.advance(8000); await flush();
  assert.equal(fixture.video.src, mp4);
  fixture.timers.advance(8000); await flush();
  assert.equal(fixture.video.src, undefined);
  assert.equal(fixture.video.paused, true);
  assert.equal(fixture.timers.pending.size, 0);
  for (const event of ["error", "canplay", "waiting"]) fixture.video.dispatchEvent(new Event(event));
  cleanup(); await flush();
  assert.deepEqual(fixture.requests, [webm, mp4]);
});

test("multi-format recovery is bounded and cancellation releases its active format", async () => {
  const formats = Array.from({ length: 20 }, (_, index) => `https://publisher.invalid/story-${index}.mp4`);
  const fixture = richVideo({ formats });
  const cleanup = fixture.attach(); await flush();
  for (let index = 0; index < 4; index++) { fixture.video.dispatchEvent(new Event("error")); await flush(); }
  assert.equal(fixture.requests.length, 4);
  assert.equal(fixture.video.src, undefined);
  cleanup();
  assert.equal(fixture.timers.pending.size, 0);

  const cancelled = richVideo();
  const cancel = cancelled.attach(); await flush();
  cancelled.video.dispatchEvent(new Event("error"));
  cancel(); await flush();
  assert.deepEqual(cancelled.requests, [webm]);
  assert.equal(cancelled.video.src, undefined);
  assert.equal(cancelled.timers.pending.size, 0);
});
