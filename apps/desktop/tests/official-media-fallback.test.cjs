const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const media = require("../src/official-media-sources.ts");
// These fallback fixtures use an immediate direct transport; cache async ownership has dedicated tests.
require.cache[path.resolve(__dirname, "../src/media-cache.ts")] = { exports: {
  createMediaCacheResolver: () => Object.assign((source) => ({ then(callback) { callback(source); } }), { disable() {} })
} };
const { createOfficialHlsLoader } = require("../src/official-hls-loader.ts");

function loadSource(relative, overrides = {}, globals = {}) {
  const filename = path.resolve(__dirname, "../src", relative);
  const exports = {};
  const nativeRequire = Module.createRequire(filename);
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, URL, Date, console, ...globals,
    require: (id) => overrides[id] ?? nativeRequire(id)
  }, { filename });
  return exports;
}

function timers() {
  const pending = new Map();
  let sequence = 0;
  return {
    pending,
    setTimeout(fn) { pending.set(++sequence, fn); return sequence; },
    clearTimeout(id) { pending.delete(id); },
    tick() { const batch = [...pending.values()]; pending.clear(); for (const fn of batch) fn(); }
  };
}

test("official media alternates preserve path, query and identity and obey purpose and path scopes", () => {
  const policy = { schemaVersion: 1, groups: [{ id: "test", origins: ["https://one.invalid", "https://two.invalid"], pathPrefixes: ["/assets/"], allowedQueryKeys: ["version", "size"], purposes: ["image"] }], exactResources: [] };
  const source = "https://one.invalid/assets/a%20b.jpg?version=4&size=large";
  assert.deepEqual(media.officialMediaCandidates(source, "image", policy), [source, source.replace("one.invalid", "two.invalid")]);
  assert.deepEqual(media.officialMediaCandidates(source, "video", policy), [source]);
  assert.deepEqual(media.officialMediaCandidates(`${source}#private`, "image", policy), [`${source}#private`]);
  assert.deepEqual(media.officialMediaCandidates(`${source}&token=private`, "image", policy), [`${source}&token=private`]);
  assert.deepEqual(media.officialMediaCandidates("https://one.invalid/private/a.jpg", "image", policy), ["https://one.invalid/private/a.jpg"]);
  for (const invalid of ["http://one.invalid/assets/a.jpg", "https://user@one.invalid/assets/a.jpg", "https://one.invalid:444/assets/a.jpg", "//one.invalid/assets/a.jpg", "javascript:alert(1)"]) {
    assert.deepEqual(media.officialMediaCandidates(invalid, "image", policy), []);
  }
  assert.deepEqual(media.officialMediaCandidates("/src/assets/cover.png", "image", policy), ["/src/assets/cover.png"]);
  assert.deepEqual(media.officialMediaCandidates("/src/assets/cover.png", "video", policy), []);
});

test("exact media mappings are bounded and never rewrite a different query or resource", () => {
  const urls = Array.from({ length: 8 }, (_, index) => `https://cdn${index}.invalid/trailer.mp4`);
  const policy = { schemaVersion: 1, groups: [], exactResources: [{ id: "movie", urls, purposes: ["video"] }] };
  assert.deepEqual(media.officialMediaCandidates(urls[0], "video", policy), urls.slice(0, 4));
  assert.deepEqual(media.officialMediaCandidates(`${urls[0]}?other=true`, "video", policy), [`${urls[0]}?other=true`]);
  assert.deepEqual(media.officialMediaCandidates(urls[0], "image", policy), [urls[0]]);
});

test("successful sources are remembered only for matching candidates with a finite TTL and capacity", () => {
  media.forgetMediaSourcePreferences();
  const sources = media.officialMediaCandidates("https://shared.akamai.steamstatic.com/steam/apps/1/a.jpg", "image");
  media.rememberMediaSource(sources, sources[1], 10, "en-US");
  assert.deepEqual(media.preferredMediaSources(sources, 20, "en-US"), [sources[1], sources[0], sources[2]]);
  assert.deepEqual(media.preferredMediaSources(["https://one.invalid/b.jpg", "https://two.invalid/b.jpg"], 20), ["https://one.invalid/b.jpg", "https://two.invalid/b.jpg"]);
  assert.deepEqual(media.preferredMediaSources(sources, 600011, "en-US"), sources);
  media.rememberMediaSource(sources, sources[1], 10, "en-US");
  for (let index = 0; index < 128; index++) {
    const other = media.officialMediaCandidates(`https://shared.akamai.steamstatic.com/steam/apps/1/${index}.jpg`, "image");
    media.rememberMediaSource(other, other[1], 10, "en-US");
  }
  assert.deepEqual(media.preferredMediaSources(sources, 20, "en-US"), sources);
  media.forgetMediaSourcePreferences();
});

function imageHook() {
  let locale = "en-US";
  const clock = timers();
  const slots = [];
  let cursor = 0;
  let effects = [];
  let dirty = false;
  let observer;
  const same = (left, right) => left?.length === right?.length && left.every((item, index) => Object.is(item, right[index]));
  const react = {
    useRef(value) { const index = cursor++; return slots[index] ??= { current: value }; },
    useState(value) { const index = cursor++; slots[index] ??= { value }; return [slots[index].value, (next) => { const value = typeof next === "function" ? next(slots[index].value) : next; dirty ||= !Object.is(value, slots[index].value); slots[index].value = value; }]; },
    useMemo(factory, deps) { const index = cursor++; if (!slots[index] || !same(slots[index].deps, deps)) slots[index] = { deps, value: factory() }; return slots[index].value; },
    useEffect(callback, deps) { const index = cursor++; if (!slots[index] || !same(slots[index].deps, deps)) effects.push(() => { slots[index]?.cleanup?.(); slots[index] = { deps, cleanup: callback() }; }); }
  };
  const { useMediaSource } = loadSource("components/useMediaSource.ts", { react, "../i18n": { useI18n: () => ({ locale }) } }, {
    window: clock,
    IntersectionObserver: class { constructor(callback) { observer = this; this.callback = callback; } observe() {} disconnect() {} }
  });
  return {
    clock,
    setLocale(next) { locale = next; },
    render(sources, lazy = false) {
      for (let count = 0; count < 10; count++) {
        cursor = 0; dirty = false;
        const result = useMediaSource(sources);
        result.ref.current = { loading: lazy ? "lazy" : "eager" };
        const pending = effects; effects = [];
        for (const run of pending) run();
        if (!dirty) return result;
      }
      assert.fail("image state did not settle");
    },
    visible() { observer.callback([{ isIntersecting: true }]); },
    cleanup() { for (const slot of slots) slot?.cleanup?.(); }
  };
}

test("image errors advance once, ignore previous resources, and stop at exhaustion", () => {
  media.forgetMediaSourcePreferences();
  const hook = imageHook();
  const first = hook.render(["/a.jpg", "/a-copy.jpg"]);
  first.onError(); first.onError();
  const second = hook.render(["/a.jpg", "/a-copy.jpg"]);
  assert.equal(second.src, "/a-copy.jpg");
  first.onError();
  assert.equal(hook.render(["/a.jpg", "/a-copy.jpg"]).src, "/a-copy.jpg");
  const other = hook.render(["/b.jpg", "/b-copy.jpg"]);
  second.onError(); second.onLoad();
  assert.equal(hook.render(["/b.jpg", "/b-copy.jpg"]).src, "/b.jpg");
  other.onError();
  hook.render(["/b.jpg", "/b-copy.jpg"]).onError();
  assert.equal(hook.render(["/b.jpg", "/b-copy.jpg"]).src, null);
  hook.cleanup();
  assert.equal(hook.clock.pending.size, 0);
});

test("lazy images wait for visibility and stalled images have bounded source deadlines", () => {
  const hook = imageHook();
  hook.render(["/slow.jpg", "/copy.jpg"], true);
  assert.equal(hook.clock.pending.size, 0);
  hook.visible();
  hook.clock.tick();
  const next = hook.render(["/slow.jpg", "/copy.jpg"], true);
  assert.equal(next.src, "/copy.jpg");
  hook.visible();
  next.onLoad();
  hook.clock.tick();
  assert.equal(hook.render(["/slow.jpg", "/copy.jpg"], true).src, "/copy.jpg");
  hook.cleanup();
  next.onError();
  assert.equal(hook.clock.pending.size, 0);
});

test("mounted images reset candidates on Chinese-English-Chinese changes and reject events from the previous language", () => {
  media.forgetMediaSourcePreferences();
  const sources = media.officialMediaCandidates("https://shared.akamai.steamstatic.com/steam/apps/1/switch.jpg", "image");
  const hook = imageHook();
  hook.setLocale("zh-CN");
  const chinese = hook.render(sources);
  assert.equal(chinese.src, sources[2]);
  chinese.onLoad();
  hook.setLocale("en-US");
  assert.equal(hook.render(sources).src, sources[0]);
  chinese.onError();
  assert.equal(hook.render(sources).src, sources[0]);
  hook.render(sources).onError();
  hook.render(sources).onLoad();
  hook.setLocale("zh-CN");
  assert.equal(hook.render(sources).src, sources[2]);
  hook.cleanup();
});

test("cancelled texture images detach handlers, stop requests and cannot start later fallbacks", () => {
  const clock = timers();
  const images = [];
  class Image { constructor() { images.push(this); } removeAttribute(name) { delete this[name]; } }
  const { loadMediaImage } = loadSource("load-media-image.ts", {}, { Image, ...clock });
  let unavailable = 0;
  const cancel = loadMediaImage(["/first.jpg", "/second.jpg"], () => assert.fail("cancelled image cannot install"), () => { unavailable++; });
  const staleError = images[0].onerror;
  clock.tick();
  assert.equal(images.length, 2);
  assert.equal(images[0].src, undefined);
  staleError();
  assert.equal(images.length, 2);
  cancel();
  assert.equal(images[1].src, undefined);
  assert.equal(images[1].onload, null);
  assert.equal(clock.pending.size, 0);
  assert.equal(unavailable, 0);
});

function hlsHarness(locale = "en-US") {
  const requests = [];
  class BaseLoader {
    context = null;
    stats = { aborted: false, loaded: 0, loading: {}, parsing: {}, buffering: {} };
    load(context, config, callbacks) { Object.assign(this, { context, config, callbacks }); requests.push(this); }
    destroy() { this.destroyed = true; }
    abort() { this.stats.aborted = true; }
    getResponseHeader(name) { return name === "age" ? "1" : null; }
  }
  const Loader = createOfficialHlsLoader(BaseLoader, locale);
  return { requests, loader: () => new Loader({}) };
}
const videoSource = "https://video.akamai.steamstatic.com/store_trailers/123/manifest.m3u8?t=a%2Bb";
const config = { loadPolicy: { errorRetry: { maxNumRetry: 2 }, timeoutRetry: { maxNumRetry: 2 } }, maxRetry: 2 };
const callbacks = (onSuccess) => ({ onSuccess, onError() { assert.fail("unexpected request failure"); }, onTimeout() { assert.fail("unexpected timeout"); } });

test("HLS region ordering applies to the initial playlist and its relative child without leaking between languages", () => {
  media.forgetMediaSourcePreferences();
  const source = videoSource.replace("123", "locale");
  for (const locale of ["zh-CN", "en-US", "zh-CN"]) {
    const harness = hlsHarness(locale);
    const manifest = harness.loader();
    let loadedUrl;
    manifest.load({ url: source, type: "manifest" }, config, callbacks((response) => { loadedUrl = response.url; }));
    const request = harness.requests[0];
    const expected = locale === "zh-CN" ? "video.cdn.steamchina.queniuam.com" : "video.akamai.steamstatic.com";
    assert.equal(new URL(request.context.url).hostname, expected);
    request.callbacks.onSuccess({ url: request.context.url, data: "#EXTM3U" }, request.stats, request.context, null);
    const child = harness.loader();
    child.load({ url: new URL("child/quality.m3u8", loadedUrl).toString(), type: "level" }, config, callbacks(() => {}));
    assert.equal(new URL(harness.requests[1].context.url).hostname, expected);
    manifest.destroy(); child.destroy();
  }
});

test("HLS retries each same-resource source and resolves relative children from the successful URL", () => {
  media.forgetMediaSourcePreferences();
  const harness = hlsHarness();
  const loader = harness.loader();
  const context = { url: videoSource, type: "manifest", responseType: "text" };
  let result;
  loader.load(context, config, callbacks((response, _stats, original) => { result = response; assert.equal(original, context); }));
  const first = harness.requests[0];
  assert.equal(first.config.maxRetry, 0);
  assert.equal(first.config.loadPolicy.errorRetry, null);
  assert.equal(first.config.loadPolicy.maxTimeToFirstByteMs, 8000);
  assert.equal(first.config.loadPolicy.maxLoadTimeMs, 8000);
  first.callbacks.onError({ code: 503, text: "offline" }, first.context, null, first.stats);
  const next = harness.requests[1];
  assert.equal(first.destroyed, true);
  assert.equal(next.context.url, videoSource.replace("video.akamai", "video.fastly"));
  next.callbacks.onSuccess({ url: next.context.url, data: "#EXTM3U\nquality/stream.m3u8" }, next.stats, next.context, null);
  const relative = new URL("quality/stream.m3u8", result.url).toString();
  assert.match(relative, /^https:\/\/video\.fastly\.steamstatic\.com\/store_trailers\/123\/quality\/stream.m3u8$/);
  const child = harness.loader();
  child.load({ ...context, url: relative }, config, callbacks(() => {}));
  assert.equal(harness.requests[2].context.url, relative);
  loader.destroy(); child.destroy();
});

test("absolute HLS fragments preserve ranges and use a successful session origin without mixing resources", () => {
  media.forgetMediaSourcePreferences();
  const harness = hlsHarness();
  const manifest = harness.loader();
  manifest.load({ url: videoSource, type: "manifest" }, config, callbacks(() => {}));
  harness.requests[0].callbacks.onTimeout({}, {}, null);
  const success = harness.requests[1];
  success.callbacks.onSuccess({ url: success.context.url, data: "#EXTM3U" }, success.stats, success.context, null);
  const context = { url: "https://video.akamai.steamstatic.com/store_trailers/123/seg-2.m4s?t=2", type: "media-fragment", responseType: "arraybuffer", rangeStart: 12, rangeEnd: 32, headers: { "X-Test": "retained" } };
  const fragment = harness.loader();
  fragment.load(context, config, callbacks((_response, _stats, original) => assert.equal(original, context)));
  const request = harness.requests[2];
  assert.equal(request.context.url, context.url.replace("video.akamai", "video.fastly"));
  assert.equal(request.context.rangeStart, 12);
  assert.equal(request.context.rangeEnd, 32);
  assert.equal(request.context.headers, context.headers);
  manifest.destroy(); fragment.destroy();
});

test("HLS exhaustion reports one error and abort or destroy rejects stale completions", () => {
  media.forgetMediaSourcePreferences();
  const harness = hlsHarness();
  const loader = harness.loader();
  let failures = 0;
  loader.load({ url: videoSource, type: "manifest" }, config, { ...callbacks(() => assert.fail("obsolete success")), onError() { failures++; } });
  for (let index = 0; index < 3; index++) {
    const request = harness.requests[index];
    request.callbacks.onError({ code: 404, text: "missing" }, request.context, null, request.stats);
  }
  assert.equal(harness.requests.length, 3);
  assert.equal(failures, 1);
  const stale = harness.requests[2];
  loader.destroy();
  stale.callbacks.onSuccess({ url: stale.context.url }, stale.stats, stale.context, null);
  const cancelled = harness.loader();
  let aborts = 0;
  cancelled.load({ url: videoSource, type: "manifest" }, config, { ...callbacks(() => assert.fail("cancelled success")), onAbort() { aborts++; } });
  const request = harness.requests.at(-1);
  cancelled.abort(); cancelled.abort();
  request.callbacks.onTimeout(request.stats, request.context, null);
  assert.equal(aborts, 1);
  assert.equal(harness.requests.at(-1), request);
  cancelled.destroy();
});

test("HLS does not bypass authentication, rate limits, Retry-After or partially delivered fragments", () => {
  for (const scenario of ["unauthorized", "forbidden", "limited", "retry-after", "progress"]) {
    media.forgetMediaSourcePreferences();
    const harness = hlsHarness();
    const loader = harness.loader();
    let failures = 0;
    loader.load({ url: videoSource, type: "media-fragment" }, config, { ...callbacks(() => {}),
      onError() { failures++; }, onProgress() {} });
    const request = harness.requests[0];
    if (scenario === "progress") request.callbacks.onProgress(request.stats, request.context, new ArrayBuffer(2), null);
    const code = { unauthorized: 401, forbidden: 403, limited: 429 }[scenario] ?? 503;
    const details = scenario === "retry-after" ? { getResponseHeader: (name) => name === "Retry-After" ? "Wed, 21 Oct 2037 07:28:00 GMT" : null } : null;
    request.callbacks.onError({ code, text: "stop" }, request.context, details, request.stats);
    assert.equal(failures, 1, scenario);
    assert.equal(harness.requests.length, 1, scenario);
    loader.destroy();
  }
});

test("sanitized rich-text media use bounded recovery and release detached images and videos", () => {
  media.forgetMediaSourcePreferences();
  const clock = timers();
  class Image extends EventTarget {
    constructor(src) { super(); this.attributes = { src }; this.complete = false; this.naturalWidth = 0; }
    getAttribute(name) { return this.attributes[name] ?? null; }
    removeAttribute(name) { delete this.attributes[name]; }
    set src(value) { this.attributes.src = value; }
    get src() { return this.attributes.src; }
  }
  const image = new Image("https://images.steamusercontent.com/ugc/123/preview.jpg?imw=100");
  let childSourceRemoved = false;
  const video = { currentSrc: "https://video.akamai.steamstatic.com/store_trailers/123/movie.mp4", autoplay: false, muted: true,
    getAttribute: () => null, querySelectorAll: () => [{ getAttribute: () => null, removeAttribute() { childSourceRemoved = true; } }] };
  let videoStopped = 0;
  const { attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
    "./stream-video-player": { attachStreamVideo(target, options) {
      assert.equal(target, video); assert.equal(options.streamUrl, video.currentSrc);
      return () => { videoStopped++; };
    } }
  }, { ...clock });
  const cleanup = attachHtmlMediaFallbacks({ querySelectorAll: (selector) => selector === "video" ? [video] : [image] });
  clock.tick();
  assert.match(image.src, /^https:\/\/steamuserimages-a\.akamaihd\.net\/ugc\/123\/preview.jpg\?imw=100$/);
  image.dispatchEvent(new Event("error"));
  assert.equal(image.hidden, true);
  assert.equal(image.src, undefined);
  cleanup();
  image.dispatchEvent(new Event("error"));
  assert.equal(clock.pending.size, 0);
  assert.equal(videoStopped, 1);
  assert.equal(childSourceRemoved, true, "cleanup cannot restart an original source child");
});

test("StrictMode rich-text effect replay recovers original image, child video and poster sources after disposal", () => {
  media.forgetMediaSourcePreferences();
  const clock = timers();
  const createdImages = [];
  class Image extends EventTarget {
    constructor(src) { super(); this.attributes = src ? { src } : {}; this.complete = false; this.naturalWidth = 0; createdImages.push(this); }
    getAttribute(name) { return this.attributes[name] ?? null; }
    removeAttribute(name) { delete this.attributes[name]; }
    set src(value) { this.attributes.src = value; }
    get src() { return this.attributes.src; }
  }
  const originalImage = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/story-fixture.png";
  const originalVideo = "https://video.akamai.steamstatic.com/store_trailers/321/movie.mp4";
  const originalPoster = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/poster.jpg";
  const image = new Image(originalImage);
  let sourceAttribute = originalVideo;
  const source = { type: "video/mp4", getAttribute: () => sourceAttribute, removeAttribute() { sourceAttribute = null; } };
  const video = { currentSrc: "", autoplay: false, muted: true, getAttribute: (name) => name === "poster" ? originalPoster : null,
    canPlayType: () => "probably", querySelectorAll: () => sourceAttribute ? [source] : [] };
  const videoStarts = [];
  let videoStops = 0;
  const { attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
    "./stream-video-player": { attachStreamVideo(_target, options) { videoStarts.push(options.streamUrl); return () => { videoStops++; }; } }
  }, { Image, ...clock });
  const container = { querySelectorAll: (selector) => selector === "video" ? [video] : [image] };
  const firstCleanup = attachHtmlMediaFallbacks(container, "en-US");
  firstCleanup();
  assert.equal(image.src, undefined);
  assert.equal(sourceAttribute, null);
  assert.equal(clock.pending.size, 0);
  const secondCleanup = attachHtmlMediaFallbacks(container, "en-US");
  assert.equal(image.src, originalImage);
  assert.deepEqual(videoStarts, [originalVideo, originalVideo]);
  assert.equal(createdImages.at(-1).src, originalPoster);
  image.dispatchEvent(new Event("error"));
  image.dispatchEvent(new Event("error"));
  assert.match(image.src, /shared\.cdn\.steamchina\.queniuam\.com/);
  image.naturalWidth = 960;
  image.complete = true;
  image.dispatchEvent(new Event("load"));
  assert.equal(image.hidden, false);
  secondCleanup();
  assert.equal(image.src, undefined);
  assert.equal(videoStops, 2);
  assert.equal(clock.pending.size, 0);
});

test("rich text inserts inert media references and starts only the chosen language source, including posters", () => {
  media.forgetMediaSourcePreferences();
  const clock = timers();
  const requests = [];
  const originalImage = "https://shared.akamai.steamstatic.com/steam/apps/1/inert-story.jpg";
  const originalPoster = "https://shared.akamai.steamstatic.com/steam/apps/1/inert-poster.jpg";
  const originalVideo = "https://video.akamai.steamstatic.com/store_trailers/inert/movie.mp4";
  class Image extends EventTarget {
    constructor(source) { super(); this.attributes = source ? { src: source } : {}; this.complete = false; this.naturalWidth = 0; }
    getAttribute(name) { return this.attributes[name] ?? null; }
    removeAttribute(name) { delete this.attributes[name]; }
    set src(source) { this.attributes.src = source; requests.push(source); }
    get src() { return this.attributes.src; }
  }
  const image = new Image(originalImage);
  const child = new Image(originalVideo);
  child.type = "video/mp4";
  const video = { attributes: { poster: originalPoster }, currentTime: 22, paused: false, autoplay: false, muted: true,
    getAttribute(name) { return this.attributes[name] ?? null; }, removeAttribute(name) { delete this.attributes[name]; },
    querySelectorAll: () => child.src ? [child] : [], canPlayType: () => "probably" };
  const content = { querySelectorAll: (selector) => selector === "video" ? [video] : [image] };
  const template = { content, innerHTML: "" };
  const container = { querySelectorAll: content.querySelectorAll,
    ownerDocument: { createElement(tag) { assert.equal(tag, "template"); return template; } },
    replaceChildren(fragment) {
      assert.equal(fragment, content);
      assert.equal(image.src, undefined);
      assert.equal(child.src, undefined);
      assert.equal(video.getAttribute("poster"), null);
      assert.deepEqual(requests, []);
    }
  };
  const videoStarts = [];
  const { replaceHtmlMediaContent, attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
    "./stream-video-player": { attachStreamVideo(_target, options) { videoStarts.push(options); return () => {}; } }
  }, { Image, ...clock });
  replaceHtmlMediaContent(container, "already sanitized fixture");
  assert.equal(template.innerHTML, "already sanitized fixture");
  let cleanup = attachHtmlMediaFallbacks(container, "zh-CN");
  assert.equal(requests.length, 2);
  assert.ok(requests.every((source) => new URL(source).hostname === "shared.cdn.steamchina.queniuam.com"));
  assert.equal(videoStarts[0].streamUrl, originalVideo);
  assert.equal(videoStarts[0].locale, "zh-CN");
  cleanup();
  cleanup = attachHtmlMediaFallbacks(container, "en-US");
  assert.equal(image.src, originalImage);
  assert.equal(videoStarts[1].locale, "en-US");
  assert.equal(videoStarts[1].initialPlayback.time, 22);
  assert.equal(videoStarts[1].initialPlayback.playing, true);
  cleanup();
  cleanup = attachHtmlMediaFallbacks(container, "zh-CN");
  assert.equal(new URL(image.src).hostname, "shared.cdn.steamchina.queniuam.com");
  cleanup();
  assert.equal(clock.pending.size, 0);
});

test("sanitized image and poster addresses outside managed candidates remain intact without invented fallbacks", () => {
  const urls = ["http://publisher.invalid/story.jpg", "https://publisher.invalid:8443/story.jpg", "https://reader@publisher.invalid/story.jpg"];
  const element = (attributes) => ({ attributes, getAttribute(name) { return this.attributes[name] ?? null; }, removeAttribute(name) { delete this.attributes[name]; } });
  for (const source of urls) {
    const image = element({ src: source });
    const video = { ...element({ src: "https://publisher.invalid/movie.mp4", poster: source }), querySelectorAll: () => [] };
    const content = { querySelectorAll: (selector) => selector === "video" ? [video] : [image] };
    const container = { querySelectorAll: content.querySelectorAll,
      ownerDocument: { createElement: () => ({ content, innerHTML: "" }) },
      replaceChildren() {
        assert.equal(image.getAttribute("src"), source);
        assert.equal(video.getAttribute("poster"), source);
      }
    };
    const { replaceHtmlMediaContent, attachHtmlMediaFallbacks } = loadSource("html-media-fallback.ts", {
      "./stream-video-player": { attachStreamVideo: () => () => {} }
    }, { Image: class { constructor() { assert.fail("unmanaged poster must not start a managed request"); } } });
    replaceHtmlMediaContent(container, "already sanitized fixture");
    for (const locale of ["zh-CN", "en-US"]) {
      const cleanup = attachHtmlMediaFallbacks(container, locale);
      assert.equal(image.getAttribute("src"), source);
      assert.equal(video.getAttribute("poster"), source);
      cleanup();
      assert.equal(image.getAttribute("src"), source);
      assert.equal(video.getAttribute("poster"), source);
    }
  }
});
