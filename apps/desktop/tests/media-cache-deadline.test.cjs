const assert = require("node:assert/strict");
const test = require("node:test");
const { loadSource, flush, clock, cacheEnvironment, ImageFixture, nativeVideo, imageHook } = require("./media-cache-test-support.cjs");
const media = require("../src/official-media-sources.ts");

const image = "https://shared.akamai.steamstatic.com/steam/apps/1/cache-deadline.jpg";
const video = "https://video.akamai.steamstatic.com/store_trailers/1/cache-deadline.mp4";
const opaque = "/__langame/media/opaque-fixture";
const imageSources = () => media.officialMediaCandidates(image, "image");

test("cached cover can finish after a slow first origin without starting a direct duplicate", async () => {
  const hook = imageHook(cacheEnvironment().cache);
  hook.render(imageSources()); await flush();
  hook.render(imageSources());
  hook.timers.advance(9000);
  await flush();
  const loaded = hook.render(imageSources());
  assert.equal(loaded.src, opaque, "the backend can still be fetching its next origin after eight seconds");
  loaded.onLoad();
  hook.timers.advance(20000); await flush();
  assert.equal(hook.render(imageSources()).src, opaque);
  hook.cleanup();
  assert.equal(hook.timers.pending.size, 0);
});

test("a stalled cover proxy remains bounded and direct candidates retain their eight-second deadline", async () => {
  media.forgetMediaSourcePreferences();
  const hook = imageHook(cacheEnvironment().cache);
  hook.render(imageSources()); await flush(); hook.render(imageSources());
  hook.timers.advance(27999); await flush();
  assert.equal(hook.render(imageSources()).src, opaque);
  hook.timers.advance(1); hook.render(imageSources()); await flush();
  assert.equal(hook.render(imageSources()).src, image);
  hook.timers.advance(7999); await flush();
  assert.equal(hook.render(imageSources()).src, image);
  hook.timers.advance(1); hook.render(imageSources()); await flush();
  assert.equal(hook.render(imageSources()).src, image.replace("shared.akamai", "shared.fastly"));
  hook.cleanup();
  assert.equal(hook.timers.pending.size, 0);
});

test("rich-text images and atmosphere textures wait for the complete cache response and cancel their deadlines", async () => {
  for (const kind of ["story", "texture"]) {
    media.forgetMediaSourcePreferences();
    const timers = clock();
    const env = cacheEnvironment();
    const images = [];
    const Image = class extends ImageFixture { constructor(source) { super(source); images.push(this); } };
    let cleanup;
    if (kind === "story") {
      const element = new Image(image);
      const { attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
        "./media-cache": env.cache, "./stream-video-player": {}
      }, timers);
      cleanup = attachHtmlMediaFallbacks({ querySelectorAll: (selector) => selector === "video" ? [] : [element] }, "en-US");
    } else {
      const { loadMediaImage } = loadSource("load-media-image.ts", { "./media-cache": env.cache }, { Image, ...timers });
      cleanup = loadMediaImage(imageSources(), () => {}, () => assert.fail("unexpected unavailable"), "en-US");
    }
    await flush();
    timers.advance(9000); await flush();
    assert.equal(images.length, 1, `${kind} must not duplicate a pending backend fetch`);
    assert.equal(images[0].src, opaque);
    cleanup();
    assert.equal(timers.pending.size, 0);
    timers.advance(30000); await flush();
    assert.equal(images[0].src, undefined);
  }
});

test("native video keeps a pending cache source for its server budget then falls back with a direct deadline", async () => {
  media.forgetMediaSourcePreferences();
  const player = nativeVideo(cacheEnvironment().cache);
  const cleanup = player.attach(video);
  await flush();
  player.timers.advance(20000); await flush();
  assert.deepEqual(player.requests, [opaque]);
  player.timers.advance(8000); await flush();
  assert.deepEqual(player.requests, [opaque, video]);
  player.timers.advance(8000); await flush();
  assert.equal(player.requests.at(-1), video.replace("video.akamai", "video.fastly"));
  cleanup();
  assert.equal(player.timers.pending.size, 0);
  assert.equal(player.failures(), 0);
});

test("HLS adds the cache server budget to the caller deadline and restores the direct limit after timeout", async () => {
  media.forgetMediaSourcePreferences();
  const timers = clock();
  const requests = [];
  class Base {
    stats = { aborted: false };
    load(context, config, callbacks) {
      Object.assign(this, { context, config, callbacks });
      requests.push(this);
      this.timer = timers.setTimeout(() => callbacks.onTimeout(this.stats, context, null), config.loadPolicy.maxLoadTimeMs);
    }
    destroy() { timers.clearTimeout(this.timer); }
    abort() { this.destroy(); }
  }
  const { createOfficialHlsLoader } = loadSource("official-hls-loader.ts", { "./media-cache": cacheEnvironment().cache });
  const Loader = createOfficialHlsLoader(Base, "en-US");
  const loader = new Loader({});
  const playlist = video.replace(".mp4", ".m3u8");
  loader.load({ url: playlist }, { loadPolicy: { maxTimeToFirstByteMs: 4000, maxLoadTimeMs: 5000 } }, {
    onSuccess() {}, onError: () => assert.fail("unexpected error"), onTimeout: () => assert.fail("unexpected exhaustion")
  });
  await flush();
  timers.advance(9000); await flush();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].config.loadPolicy.maxTimeToFirstByteMs, 24000);
  assert.equal(requests[0].config.loadPolicy.maxLoadTimeMs, 25000);
  timers.advance(16000); await flush();
  assert.equal(requests[1].context.url, playlist);
  assert.equal(requests[1].config.loadPolicy.maxTimeToFirstByteMs, 4000);
  assert.equal(requests[1].config.loadPolicy.maxLoadTimeMs, 5000);
  loader.destroy();
  assert.equal(timers.pending.size, 0);
});
