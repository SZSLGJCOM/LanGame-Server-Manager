const assert = require("node:assert/strict");
const test = require("node:test");
const { cacheEnvironment, deferred } = require("./media-cache-test-support.cjs");
const image = "https://shared.akamai.steamstatic.com/steam/apps/1/cache.jpg?t=1";
const video = "https://video.akamai.steamstatic.com/store_trailers/1/cache.mp4";

test("Tauri image and HLS cache URLs contain only official media identity, kind and locale without an IPC registration", async () => {
  const env = cacheEnvironment({ tauri: true });
  for (const [source, kind, locale] of [[image, "image", "zh-CN"], [video.replace("mp4", "m3u8"), "hls", "zh-CN"]]) {
    const url = new URL(await env.cache.resolveMediaCacheSource(source, kind, locale));
    assert.equal(url.origin, "http://lgsm-media.localhost");
    assert.equal(url.searchParams.get("url"), source);
    assert.equal(url.searchParams.get("kind"), kind);
    assert.equal(url.searchParams.get("locale"), locale);
    assert.deepEqual([...url.searchParams.keys()], ["url", "kind", "locale"]);
  }
  assert.equal(env.calls.length, 0);
});

test("native video registers one opaque playback lease per lifecycle in both Tauri and LAN, never a query cache URL", async () => {
  for (const tauri of [true, false]) {
    let registrations = 0;
    const env = cacheEnvironment({ tauri, register: async () => `/__langame/media/lease-${++registrations}` });
    const resolve = env.cache.createMediaCacheResolver("video", "zh-CN");
    const first = resolve(video);
    const replay = resolve(video);
    const alias = video.replace("video.akamai", "video.fastly");
    const alternate = resolve(alias);
    assert.equal(env.calls.length, 1);
    assert.deepEqual(env.calls[0], [video, "video", "zh-CN"]);
    const prefix = tauri ? "http://lgsm-media.localhost" : "";
    const expected = `${prefix}/__langame/media/lease-1`;
    assert.equal(await first, expected);
    assert.equal(await replay, expected);
    assert.equal(await alternate, expected);
    assert.equal(expected.includes("?"), false);
    resolve.disable(video);
    assert.equal(await resolve(video), video, "a failed or expired lease restores the direct source");
    assert.equal(await resolve(alias), alias);
    assert.equal(env.calls.length, 1, "failure must not renew the same playback lease");
    const nextPlayback = env.cache.createMediaCacheResolver("video", "zh-CN");
    assert.equal(await nextPlayback(video), `${prefix}/__langame/media/lease-2`);
    assert.equal(env.calls.length, 2);
  }
});

test("Tauri native video waits for registration and uses the original URL after timeout or invalid lease", async () => {
  const pending = deferred();
  const env = cacheEnvironment({ tauri: true, register: () => pending.promise });
  const result = env.cache.resolveMediaCacheSource(video, "video", "en-US");
  assert.equal(env.calls.length, 1);
  assert.equal(env.timers.pending.size, 1);
  env.timers.tick();
  assert.equal(await result, video);
  pending.resolve("/__langame/media/late-lease");
  assert.equal(await result, video);
  for (const registered of [null, "https://other.invalid/lease", "/__langame/media/lease?url=video", "http://lgsm-media.localhost/?url=video&kind=video"]) {
    const invalid = cacheEnvironment({ tauri: true, register: async () => registered });
    assert.equal(await invalid.cache.resolveMediaCacheSource(video, "video", "en-US"), video);
  }
  for (const source of [`${video}?token=private`, `${video}#private`, "https://unknown.invalid/movie.mp4"]) {
    assert.equal(await env.cache.resolveMediaCacheSource(source, "video", "en-US"), source);
  }
  assert.equal(env.calls.length, 1, "unmapped video references cannot register a lease");
});

test("LAN registration accepts only opaque local endpoints and leaves non-equivalent resources unregistered", async () => {
  const env = cacheEnvironment();
  assert.equal(await env.cache.resolveMediaCacheSource(image, "image", "zh-CN"), "/__langame/media/opaque-fixture");
  assert.deepEqual(env.calls[0], [image, "image", "zh-CN"]);
  for (const [source, kind] of [[`${image}&token=private`, "image"], [`${image}#private`, "image"], ["https://unknown.invalid/image.jpg", "image"], ["/local.jpg", "image"], [image.replace("https://", "https://user@"), "image"], [image.replace(".com/", ".com:8443/"), "image"], [image, "video"]]) {
    assert.equal(await env.cache.resolveMediaCacheSource(source, kind, "en-US"), source);
  }
  assert.equal(env.calls.length, 1);
  for (const response of [null, "https://other.invalid/cache", "/__langame/media/key?token=secret", "//other.invalid/media/key", "/__langame/media/../secret"]) {
    const invalid = cacheEnvironment({ register: async () => response });
    assert.equal(await invalid.cache.resolveMediaCacheSource(image, "image", "en-US"), image);
  }
});

test("unavailable registration has a bounded wait, resolves directly, and rejects late results", async () => {
  const pending = deferred();
  const env = cacheEnvironment({ register: () => pending.promise });
  const result = env.cache.resolveMediaCacheSource(image, "image", "en-US");
  env.timers.tick();
  assert.equal(await result, image);
  pending.resolve("/__langame/media/late");
  assert.equal(await result, image);
  assert.equal(env.timers.pending.size, 0);
  const rejected = cacheEnvironment({ register: async () => { throw new Error("offline"); } });
  assert.equal(await rejected.cache.resolveMediaCacheSource(image, "image", "en-US"), image);
  assert.equal(rejected.timers.pending.size, 0);
});

test("one lifecycle shares a cache attempt across CDN aliases and effect replay, then permanently uses direct aliases after failure", async () => {
  const pending = deferred();
  const env = cacheEnvironment({ register: () => pending.promise });
  const resolve = env.cache.createMediaCacheResolver("image", "zh-CN");
  const first = resolve(image);
  const replay = resolve(image);
  const alias = image.replace("shared.akamai", "shared.fastly");
  const equivalent = resolve(alias);
  assert.equal(env.calls.length, 1);
  pending.resolve("/__langame/media/shared");
  assert.equal(await first, "/__langame/media/shared");
  assert.equal(await replay, "/__langame/media/shared");
  assert.equal(await equivalent, "/__langame/media/shared");
  resolve.disable(image);
  assert.equal(await resolve(image), image);
  assert.equal(await resolve(alias), alias);
  assert.equal(env.calls.length, 1);
  assert.equal(await resolve(image.replace("cache.jpg", "cover.jpg")), "/__langame/media/shared");
  assert.equal(env.calls.length, 2, "a different fallback cover is a distinct cache resource");
});
