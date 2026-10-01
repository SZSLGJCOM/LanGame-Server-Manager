const assert = require("node:assert/strict");
const test = require("node:test");
const { loadSource, flush, deferred, clock, cacheEnvironment, ImageFixture, nativeVideo, imageHook } = require("./media-cache-test-support.cjs");
const media = require("../src/official-media-sources.ts");
const image = "https://shared.akamai.steamstatic.com/steam/apps/1/async-cache.jpg";
const video = "https://video.akamai.steamstatic.com/store_trailers/1/async-cache.mp4";
const opaque = "/__langame/media/opaque-fixture";
const imageSources = () => media.officialMediaCandidates(image, "image");

test("image hook waits for cache registration, shares StrictMode replay, and falls back directly only once", async () => {
  media.forgetMediaSourcePreferences();
  const pending = deferred();
  const env = cacheEnvironment({ register: () => pending.promise });
  const hook = imageHook(env.cache);
  assert.equal(hook.render(imageSources()).src, null);
  hook.replay();
  assert.equal(env.calls.length, 1);
  pending.resolve(opaque);
  await flush();
  const cached = hook.render(imageSources());
  assert.equal(cached.src, opaque);
  cached.onError(); cached.onError();
  assert.equal(hook.render(imageSources()).src, null);
  await flush();
  const direct = hook.render(imageSources());
  assert.equal(direct.src, image);
  cached.onLoad(); cached.onError();
  assert.equal(hook.render(imageSources()).src, image);
  direct.onError(); hook.render(imageSources()); await flush();
  assert.equal(hook.render(imageSources()).src, image.replace("shared.akamai", "shared.fastly"));
  assert.equal(env.calls.length, 1);
  hook.cleanup();
  assert.equal(hook.timers.pending.size, 0);
});

test("image hook rejects old resource and language resolutions, including after unmount", async () => {
  media.forgetMediaSourcePreferences();
  const pending = [];
  const env = cacheEnvironment({ register: () => { const value = deferred(); pending.push(value); return value.promise; } });
  const hook = imageHook(env.cache);
  hook.render(imageSources());
  hook.setLocale("zh-CN");
  hook.render(imageSources());
  pending[0].resolve("/__langame/media/obsolete-en");
  await flush();
  assert.equal(hook.render(imageSources()).src, null);
  pending[1].resolve("/__langame/media/current-zh");
  await flush();
  assert.equal(hook.render(imageSources()).src, "/__langame/media/current-zh");
  const other = media.officialMediaCandidates(image.replace("async-cache", "other"), "image");
  hook.render(other);
  hook.cleanup();
  pending[2].resolve("/__langame/media/unmounted");
  await flush();
  assert.equal(hook.timers.pending.size, 0);
});

test("texture image requests begin with the cache URL and preserve the remote identity on success", async () => {
  media.forgetMediaSourcePreferences();
  const env = cacheEnvironment();
  const timers = clock();
  const images = [];
  const Image = class extends ImageFixture { constructor() { super(); images.push(this); } };
  const { loadMediaImage } = loadSource("load-media-image.ts", { "./media-cache": env.cache }, { Image, ...timers });
  let loaded;
  const cleanup = loadMediaImage(imageSources(), (value, source) => { loaded = { value, source }; }, () => assert.fail("unexpected failure"), "en-US");
  assert.equal(images.length, 0);
  await flush();
  assert.deepEqual(images[0].requests, [opaque]);
  images[0].emit("error");
  await flush();
  assert.equal(images[0].src, undefined);
  assert.equal(images[1].src, image);
  images[1].emit("load");
  assert.equal(loaded.source, image);
  assert.equal(loaded.value, images[1]);
  assert.equal(env.calls.length, 1);
  cleanup();
});

test("cancelled texture registration cannot create an image or a later fallback", async () => {
  const pending = deferred();
  const env = cacheEnvironment({ register: () => pending.promise });
  const { loadMediaImage } = loadSource("load-media-image.ts", { "./media-cache": env.cache }, {
    Image: class { constructor() { assert.fail("cancelled texture created an image"); } }
  });
  const cleanup = loadMediaImage(imageSources(), () => assert.fail("obsolete texture loaded"), () => assert.fail("obsolete texture failed"), "en-US");
  cleanup();
  pending.resolve(opaque);
  await flush();
});

test("rich text waits for cache URLs and obsolete bindings never restore a direct or cached source", async () => {
  media.forgetMediaSourcePreferences();
  const pending = [];
  const env = cacheEnvironment({ register: () => { const value = deferred(); pending.push(value); return value.promise; } });
  const timers = clock();
  const element = new ImageFixture(image);
  const { attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", { "./media-cache": env.cache, "./stream-video-player": {} }, timers);
  const container = { querySelectorAll: (selector) => selector === "video" ? [] : [element] };
  const old = attachHtmlMediaFallbacks(container, "en-US");
  assert.equal(element.src, undefined);
  old();
  const cleanup = attachHtmlMediaFallbacks(container, "zh-CN");
  pending[0].resolve("/__langame/media/old"); await flush();
  assert.deepEqual(element.requests, []);
  pending[1].resolve(opaque); await flush();
  assert.deepEqual(element.requests, [opaque]);
  element.emit("error"); await flush();
  assert.equal(new URL(element.src).hostname, "shared.cdn.steamchina.queniuam.com");
  assert.equal(env.calls.length, 2, "one cache registration per attached locale");
  cleanup();
  element.emit("error");
  assert.equal(element.src, undefined);
  assert.equal(timers.pending.size, 0);
});

test("native video starts through the cache, retains its playhead after cache failure and never re-registers an alternate", async () => {
  media.forgetMediaSourcePreferences();
  const env = cacheEnvironment();
  const player = nativeVideo(env.cache);
  const cleanup = player.attach(video);
  assert.equal(player.video.src, undefined);
  await flush();
  assert.deepEqual(player.requests, [opaque]);
  player.video.currentTime = 36;
  player.video.paused = false;
  player.video.dispatchEvent(new Event("canplay"));
  player.video.dispatchEvent(new Event("error"));
  assert.equal(player.video.src, undefined);
  await flush();
  assert.equal(player.video.src, video);
  player.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(player.video.currentTime, 36);
  player.video.dispatchEvent(new Event("error")); await flush();
  assert.equal(player.video.src, video.replace("video.akamai", "video.fastly"));
  player.video.dispatchEvent(new Event("loadedmetadata"));
  assert.equal(player.video.currentTime, 36);
  assert.equal(env.calls.length, 1);
  assert.equal(env.calls[0][1], "video");
  assert.equal(player.failures(), 0);
  cleanup();
});

test("unmounted native video ignores pending cache registration and late playback events", async () => {
  const pending = deferred();
  const env = cacheEnvironment({ register: () => pending.promise });
  const player = nativeVideo(env.cache);
  const cleanup = player.attach(video);
  cleanup();
  pending.resolve(opaque); await flush();
  player.video.dispatchEvent(new Event("canplay"));
  assert.deepEqual(player.requests, []);
  assert.equal(player.timers.pending.size, 0);
  assert.equal(player.failures(), 0);
});

test("a rejected play promise from the abandoned cache source cannot fail its direct replacement", async () => {
  media.forgetMediaSourcePreferences();
  const player = nativeVideo(cacheEnvironment().cache);
  const pending = deferred();
  player.video.play = () => pending.promise;
  const cleanup = player.attach(video);
  await flush();
  player.video.dispatchEvent(new Event("canplay"));
  player.video.dispatchEvent(new Event("error"));
  await flush();
  assert.equal(player.video.src, video);
  pending.reject(new Error("abandoned cache playback"));
  await flush();
  assert.equal(player.failures(), 0);
  assert.equal(player.video.src, video);
  cleanup();
});

function hls(env) {
  const requests = [];
  class Base {
    stats = { aborted: false };
    headers = {};
    load(context, config, callbacks) { Object.assign(this, { context, config, callbacks }); requests.push(this); }
    getResponseHeader(name) { return this.headers[name] ?? null; }
    destroy() { this.destroyed = true; }
    abort() { this.aborted = true; }
  }
  const { createOfficialHlsLoader } = loadSource("official-hls-loader.ts", { "./media-cache": env.cache });
  const Loader = createOfficialHlsLoader(Base, "en-US");
  return { requests, loader: () => new Loader({}) };
}
const hlsConfig = { loadPolicy: {} };
const playlist = video.replace(".mp4", ".m3u8");
const callbacks = (success = () => {}) => ({ onSuccess: success, onError: () => assert.fail("unexpected error"), onTimeout: () => assert.fail("unexpected timeout") });

test("cached HLS preserves Range context and uses its verified remote response header for child URLs", async () => {
  media.forgetMediaSourcePreferences();
  const env = cacheEnvironment();
  const harness = hls(env);
  const loader = harness.loader();
  const remote = playlist.replace("video.akamai", "video.fastly");
  const context = { url: playlist, type: "manifest", rangeStart: 12, rangeEnd: 34 };
  let response;
  loader.load(context, hlsConfig, callbacks((value, _stats, original) => { response = value; assert.equal(original, context); }));
  assert.equal(harness.requests.length, 0);
  await flush();
  const request = harness.requests[0];
  assert.equal(request.context.url, opaque);
  assert.equal(request.context.rangeStart, 12);
  assert.equal(request.context.rangeEnd, 34);
  assert.equal(env.calls[0][1], "hls");
  request.headers["X-LanGame-Media-Origin"] = remote;
  request.callbacks.onSuccess({ url: "http://localhost/__langame/media/opaque-fixture", data: "#EXTM3U" }, request.stats, request.context, null);
  assert.equal(response.url, remote);
  assert.equal(new URL("quality/child.m3u8", response.url).origin, new URL(remote).origin);
  loader.destroy();
});

test("cached HLS without a valid origin header uses the remote candidate, never the local proxy as a relative base", async () => {
  for (const header of [null, "https://different.invalid/movie.m3u8"]) {
    media.forgetMediaSourcePreferences();
    const harness = hls(cacheEnvironment());
    const loader = harness.loader();
    let response;
    loader.load({ url: playlist }, hlsConfig, callbacks((value) => { response = value; }));
    await flush();
    const request = harness.requests[0];
    request.headers["X-LanGame-Media-Origin"] = header;
    request.callbacks.onSuccess({ url: opaque, data: "#EXTM3U" }, request.stats, request.context, null);
    assert.equal(response.url, playlist);
    loader.destroy();
  }
});

test("HLS cache failure recovers to one direct attempt per alias without re-registering the same key", async () => {
  media.forgetMediaSourcePreferences();
  const env = cacheEnvironment();
  const harness = hls(env);
  const loader = harness.loader();
  loader.load({ url: playlist }, hlsConfig, callbacks()); await flush();
  let request = harness.requests[0];
  request.callbacks.onError({ code: 503, text: "cache unavailable" }, request.context, null, request.stats); await flush();
  assert.equal(harness.requests[1].context.url, playlist);
  assert.equal(request.destroyed, true);
  request = harness.requests[1];
  request.callbacks.onError({ code: 503, text: "origin unavailable" }, request.context, null, request.stats); await flush();
  assert.equal(harness.requests[2].context.url, playlist.replace("video.akamai", "video.fastly"));
  assert.equal(env.calls.length, 1);
  loader.destroy();
});

test("HLS does not bypass a cache rate limit and destruction cancels pending registration ownership", async () => {
  const env = cacheEnvironment();
  const harness = hls(env);
  const loader = harness.loader();
  let failures = 0;
  loader.load({ url: playlist }, hlsConfig, { ...callbacks(), onError: () => { failures++; } }); await flush();
  const request = harness.requests[0];
  request.callbacks.onError({ code: 429, text: "limited" }, request.context, null, request.stats); await flush();
  assert.equal(failures, 1);
  assert.equal(harness.requests.length, 1);
  loader.destroy();
  const pending = deferred();
  const cancelled = hls(cacheEnvironment({ register: () => pending.promise }));
  const abandoned = cancelled.loader();
  abandoned.load({ url: playlist }, hlsConfig, callbacks(() => assert.fail("obsolete success")));
  abandoned.destroy();
  pending.resolve(opaque); await flush();
  assert.equal(cancelled.requests.length, 0);
});
