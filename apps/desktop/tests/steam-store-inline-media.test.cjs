const assert = require("node:assert/strict");
const test = require("node:test");
const { cacheEnvironment } = require("./media-cache-test-support.cjs");
const media = require("../src/official-media-sources.ts");

const base = "/store_item_assets/steam/apps/322330/extras/ab42e93c5610ff5c234e67ae9f3a8c4a";
const origins = [
  "https://shared.akamai.steamstatic.com",
  "https://shared.fastly.steamstatic.com",
  "https://shared.cdn.steamchina.queniuam.com"
];

test("Steam inline WebM and MP4 use the verified shared CDN aliases and locale ordering", () => {
  media.forgetMediaSourcePreferences();
  for (const extension of ["webm", "mp4"]) {
    const expected = origins.map((origin) => `${origin}${base}.${extension}?t=1790287771`);
    const candidates = media.officialMediaCandidates(expected[0], "video");
    assert.deepEqual(candidates, expected);
    assert.deepEqual(media.preferredMediaSources(candidates, 0, "zh-CN"), [expected[2], expected[0], expected[1]]);
    assert.deepEqual(media.preferredMediaSources(candidates, 0, "en-US"), expected);
    for (const source of expected) assert.equal(media.isOfficialMediaUrl(source, "video"), true);
  }
});

test("inline videos register a native playback lease and CDN aliases share it", async () => {
  for (const tauri of [true, false]) {
    const env = cacheEnvironment({ tauri });
    const resolve = env.cache.createMediaCacheResolver("video", "zh-CN");
    for (const extension of ["webm", "mp4"]) {
      const sources = origins.map((origin) => `${origin}${base}.${extension}?t=1790287771`);
      const expected = `${tauri ? "http://lgsm-media.localhost" : ""}/__langame/media/opaque-fixture`;
      for (const source of sources) assert.equal(await resolve(source), expected);
    }
    assert.equal(env.calls.length, 2, "different encodings retain separate playback leases");
  }
});

test("inline video support preserves image purposes and existing URL boundaries", () => {
  const poster = `${origins[0]}${base}.poster.avif?t=1790287771`;
  assert.equal(media.officialMediaCandidates(poster, "image").length, 3);
  for (const source of [
    poster, `${origins[0]}${base}.jpg`, `${origins[0]}/private/movie.webm`,
    `${origins[0]}/steam/apps/322330/extras/movie.webm`, `${origins[0]}${base}.WEBM`,
    `${origins[0]}/store_item_assets/Steam/apps/322330/extras/movie.webm`, `${origins[0]}${base}.w%65bm`
  ]) {
    assert.deepEqual(media.officialMediaCandidates(source, "video"), [source]);
    assert.equal(media.isOfficialMediaUrl(source, "video"), false);
  }
  for (const source of [
    `${origins[0]}${base}.webm?token=private`,
    `${origins[0]}${base}.mp4#private`,
    `${origins[0]}${base}.webm?t=1&t=2`,
    `https://unknown.invalid${base}.webm`
  ]) {
    assert.deepEqual(media.officialMediaCandidates(source, "video"), [source]);
    assert.equal(media.isOfficialMediaUrl(source, "video"), false);
  }
});
