const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const media = require("../src/official-media-sources.ts");

function loadSource(relative, overrides = {}, globals = {}) {
  const filename = path.resolve(__dirname, "../src", relative);
  const exports = {};
  const nativeRequire = Module.createRequire(filename);
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, URL, Date, console, ...globals, require: (id) => overrides[id] ?? nativeRequire(id)
  }, { filename });
  return exports;
}

test("locale reads saved preferences before render and follows browser language only without a saved choice", () => {
  const storage = new Map();
  const window = { localStorage: { getItem: (key) => storage.get(key), setItem: (key, value) => storage.set(key, value) }, navigator: { language: "en-GB" } };
  const locale = loadSource("locale-preference.ts", {}, { window });
  assert.equal(locale.readPreferredLocale(), "en-US");
  window.navigator.language = "zh-TW";
  assert.equal(locale.readPreferredLocale(), "zh-CN");
  locale.writePreferredLocale("en-US");
  assert.equal(locale.readPreferredLocale(), "en-US");
  locale.writePreferredLocale("zh-CN");
  assert.equal(locale.readPreferredLocale(), "zh-CN");
  assert.equal(storage.size, 1);
  assert.equal(storage.get("langame.locale"), "zh-CN");
  assert.equal(loadSource("locale-preference.ts").readPreferredLocale(), "zh-CN");
});

test("I18n language changes synchronously update the request preference before effects or the next render", () => {
  const storage = new Map([["langame.locale", "zh-CN"]]);
  const window = { localStorage: { getItem: (key) => storage.get(key), setItem: (key, value) => storage.set(key, value) }, navigator: { language: "en-US" } };
  const locale = loadSource("locale-preference.ts", {}, { window });
  let cursor = 0;
  let observed;
  const effects = [];
  const react = {
    useState(initial) {
      const index = cursor++;
      const value = index === 1 ? { "zh-CN": {}, "en-US": {} } : typeof initial === "function" ? initial() : initial;
      return [value, (next) => { if (index === 0) observed = { next, requestLocale: locale.readPreferredLocale() }; }];
    },
    useCallback: (callback) => callback, useMemo: (factory) => factory(), useEffect: (effect) => effects.push(effect)
  };
  const { I18nProvider } = loadSource("i18n.tsx", {
    react, "react/jsx-runtime": { jsx: (type, props) => ({ type, props }) },
    "./locale-preference": locale, "./i18n-context": { I18nContext: { Provider: "provider" } },
    "./i18n-catalog-loader": { createCatalogLoader: () => ({}) }
  }, { window, document: { documentElement: { lang: "" } } });
  const result = I18nProvider({ children: "content" });
  assert.equal(result.props.value.locale, "zh-CN");
  result.props.value.setLocale("en-US");
  assert.deepEqual(observed, { next: "en-US", requestLocale: "en-US" });
  // The previous render's document effect cannot overwrite the persisted new preference.
  effects[0]();
  assert.equal(locale.readPreferredLocale(), "en-US");
  result.props.value.setLocale("zh-CN");
  assert.deepEqual(observed, { next: "zh-CN", requestLocale: "zh-CN" });
});

const image = (name) => media.officialMediaCandidates(`https://shared.akamai.steamstatic.com/steam/apps/1/${name}.jpg`, "image");

test("language priority beats foreign successes and limits failures to a locale-specific cooldown", () => {
  media.forgetMediaSourcePreferences();
  const sources = image("regional");
  const [akamai, fastly, china] = sources;
  assert.deepEqual(media.preferredMediaSources(sources, 10, "zh-CN"), [china, akamai, fastly]);
  media.rememberMediaSource(sources, fastly, 10, "zh-CN");
  assert.deepEqual(media.preferredMediaSources(sources, 20, "zh-CN"), [china, fastly, akamai]);
  assert.deepEqual(media.preferredMediaSources(sources, 20, "en-US"), sources);
  media.rememberMediaSource(sources, china, 10, "en-US");
  assert.deepEqual(media.preferredMediaSources(sources, 20, "en-US"), sources);
  media.rememberMediaSourceFailure(sources, china, 20, "zh-CN");
  assert.deepEqual(media.preferredMediaSources(sources, 21, "zh-CN"), [fastly, akamai, china]);
  assert.deepEqual(media.preferredMediaSources(sources, 21, "en-US"), sources);
  assert.deepEqual(media.preferredMediaSources(sources, 20 + media.MEDIA_SOURCE_COOLDOWN_MS, "zh-CN"), [china, fastly, akamai]);
});

test("successful fallback covers never displace a screenshot and signed or unknown URLs stay untouched", () => {
  media.forgetMediaSourcePreferences();
  const screenshot = image("current");
  const cover = image("cover");
  const sources = [...screenshot, ...cover, "/local-cover.png"];
  media.rememberMediaSource(sources, cover[2], 10, "zh-CN");
  assert.deepEqual(media.preferredMediaSources(sources, 20, "zh-CN"), [screenshot[2], screenshot[0], screenshot[1], cover[2], cover[0], cover[1], "/local-cover.png"]);
  for (const suffix of ["?token=private", "#private"]) {
    const signed = screenshot.map((url) => `${url}${suffix}`);
    media.rememberMediaSource(signed, signed[2], 10, "zh-CN");
    assert.deepEqual(media.preferredMediaSources(signed, 20, "zh-CN"), signed);
  }
  const unknown = ["https://one.invalid/a.jpg", "https://two.invalid/a.jpg"];
  media.rememberMediaSource(unknown, unknown[1], 10, "zh-CN");
  assert.deepEqual(media.preferredMediaSources(unknown, 20, "zh-CN"), unknown);
});
