const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");
const modulesDir = path.join(root, "modules");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module, filename) {
  module._compile("", filename);
};
require.extensions[".png"] = function compileImageAsset(module, filename) {
  module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);
};
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveViteRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

const {
  buildLibraryDetailProfile,
  createLibraryDetailResourceCache,
  formatSteamNewsDigest,
  formatSteamReviewSummary,
  shouldFetchSteamReviewSummary
} = require(path.join(desktopRoot, "src", "views", "library", "library-detail-intelligence.ts"));
const { getLocalizedModuleStoreData, getModuleStoreData } = require(path.join(desktopRoot, "src", "store-media.ts"));
const { ZH_CN_MODULE_STORE_COPY } = require(path.join(desktopRoot, "src", "store-copy.ts"));
const api = require(path.join(desktopRoot, "src", "api.ts"));

function realModuleIds() {
  return fs.readdirSync(modulesDir)
    .filter((moduleId) => fs.existsSync(path.join(modulesDir, moduleId, "module.toml")))
    .filter((moduleId) => fs.existsSync(path.join(modulesDir, moduleId, "schema.json")))
    .sort();
}

test("Steam detail profiles expose review and news capability", () => {
  const storeEntry = getLocalizedModuleStoreData("palworld", "zh-CN");
  const profile = buildLibraryDetailProfile("palworld", storeEntry);

  assert.equal(profile.moduleId, "palworld");
  assert.equal(profile.storeSource, "steam");
  assert.equal(profile.steamAppId, 1623730);
  assert.equal(profile.canFetchSteamNews, true);
  assert.equal(profile.canFetchSteamReviewSummary, true);
  assert.equal(shouldFetchSteamReviewSummary(profile), true);
  assert.equal(profile.officialLinks.length, 0);
});

test("official-source detail profiles use curated links instead of Steam calls", () => {
  const storeEntry = getLocalizedModuleStoreData("minecraft", "zh-CN");
  const profile = buildLibraryDetailProfile("minecraft", storeEntry);

  assert.equal(profile.storeSource, "official");
  assert.equal(profile.steamAppId, null);
  assert.equal(profile.canFetchSteamNews, false);
  assert.equal(profile.canFetchSteamReviewSummary, false);
  assert.equal(shouldFetchSteamReviewSummary(profile), false);
  assert.ok(profile.officialLinks.length >= 3);
  assert.equal(profile.hasMedia, true);
  assert.equal(profile.screenshotCount, 4);
  assert.equal(profile.trailerCount, 1);
  assert.equal(profile.mediaCount, 5);
  assert.ok(storeEntry.aboutParagraphs.length >= 4);
});

test("review summaries are formatted without exposing individual review text", () => {
  const formatted = formatSteamReviewSummary(
    {
      app_id: 1623730,
      review_score: 8,
      review_score_desc: "Very Positive",
      total_positive: 8700,
      total_negative: 1300,
      total_reviews: 10000,
      positive_percent: 87,
      source_url: "https://store.steampowered.com/app/1623730/#app_reviews_hash"
    },
    "en-US"
  );

  assert.equal(formatted.scoreLabel, "Very Positive");
  assert.equal(formatted.positivePercentLabel, "87%");
  assert.equal(formatted.totalReviewsLabel, "10,000");
  assert.equal(formatted.sourceUrl, "https://store.steampowered.com/app/1623730/#app_reviews_hash");
  assert.equal(Object.hasOwn(formatted, "reviews"), false);
});

test("Steam news digest sorts, dedupes, and cleans update rows", () => {
  const digest = formatSteamNewsDigest(
    [
      {
        gid: "older",
        title: "Older notes",
        url: "https://store.steampowered.com/news/app/1/view/older",
        author: "Studio",
        feed_label: "",
        excerpt: "<p>Older&nbsp;build&nbsp;notes</p>",
        published_at_unix_ms: Date.UTC(2026, 0, 1)
      },
      {
        gid: "latest",
        title: "Stable patch",
        url: "https://store.steampowered.com/news/app/1/view/latest",
        author: "Studio",
        feed_label: "Steam News",
        excerpt: "<b>Ports</b>, memory tuning, and server browser fixes landed.",
        published_at_unix_ms: Date.UTC(2026, 1, 1)
      },
      {
        gid: "duplicate-url",
        title: "Duplicate patch",
        url: "https://store.steampowered.com/news/app/1/view/latest",
        author: "Studio",
        feed_label: "Steam News",
        excerpt: "Duplicate copy should not be shown.",
        published_at_unix_ms: Date.UTC(2026, 1, 2)
      }
    ],
    "en-US",
    2
  );

  assert.deepEqual(digest.map((item) => item.title), ["Stable patch", "Older notes"]);
  assert.equal(digest[0].sourceLabel, "Steam News");
  assert.equal(digest[0].excerpt, "Ports, memory tuning, and server browser fixes landed.");
  assert.doesNotMatch(digest[0].excerpt, /<|>|&nbsp;/);
});

test("detail resource cache reuses fresh in-flight requests", async () => {
  let now = 1000;
  let calls = 0;
  const cache = createLibraryDetailResourceCache({ ttlMs: 5000, now: () => now });

  const first = cache.read("news:1", async () => {
    calls += 1;
    return { calls };
  });
  const second = cache.read("news:1", async () => {
    calls += 1;
    return { calls };
  });

  assert.equal(first, second);
  assert.deepEqual(await second, { calls: 1 });
  assert.equal(calls, 1);

  now = 7000;
  assert.deepEqual(await cache.read("news:1", async () => {
    calls += 1;
    return { calls };
  }), { calls: 2 });
});

test("detail resource cache drops failed requests so callers can retry", async () => {
  let calls = 0;
  const cache = createLibraryDetailResourceCache({ ttlMs: 5000, now: () => 1000 });

  await assert.rejects(
    cache.read("review:1", async () => {
      calls += 1;
      throw new Error("temporary failure");
    }),
    /temporary failure/
  );

  assert.equal(await cache.read("review:1", async () => {
    calls += 1;
    return "ok";
  }), "ok");
  assert.equal(calls, 2);
});

test("story cache drops empty results even without a mounted consumer", async () => {
  const cache = createLibraryDetailResourceCache({ ttlMs: 5000, shouldCache: Boolean });
  let finish;
  const pending = cache.read("story:zh-CN:1", () => new Promise((resolve) => { finish = resolve; }));
  await Promise.resolve();
  finish(null);
  await pending;
  assert.equal(await cache.read("story:zh-CN:1", async () => "recovered"), "recovered");
});

test("late empty stories cannot evict a newer retry and story cache stays bounded", async () => {
  const cache = createLibraryDetailResourceCache({ ttlMs: 5000, shouldCache: Boolean, maxEntries: 2 });
  let finish;
  const old = cache.read("story:1", () => new Promise((resolve) => { finish = resolve; }));
  await Promise.resolve();
  cache.clear("story:1");
  assert.equal(await cache.read("story:1", async () => "recovered"), "recovered");
  finish(null);
  await old;
  assert.equal(await cache.read("story:1", async () => "incorrect refetch"), "recovered");
  await cache.read("story:2", async () => "two");
  await cache.read("story:3", async () => "three");
  assert.equal(await cache.read("story:1", async () => "evicted and fetched"), "evicted and fetched");
});

test("all real modules build a complete local detail profile", () => {
  const incomplete = realModuleIds()
    .map((moduleId) => buildLibraryDetailProfile(moduleId, getLocalizedModuleStoreData(moduleId, "zh-CN")))
    .filter((profile) => profile.missing.length > 0)
    .map((profile) => `${profile.moduleId}: ${profile.missing.join(", ")}`);

  assert.deepEqual(incomplete, []);
});

test("all real modules expose intro, media, review, and update source coverage", () => {
  const gaps = realModuleIds()
    .map((moduleId) => buildLibraryDetailProfile(moduleId, getLocalizedModuleStoreData(moduleId, "zh-CN")))
    .flatMap((profile) => profile.detailContentIssues.map((issue) => `${profile.moduleId}: ${issue}`));

  assert.deepEqual(gaps, []);
});

test("all real zh-CN detail pages expose complete game introductions, not only hosting notes", () => {
  const gaps = realModuleIds()
    .map((moduleId) => {
      const storeEntry = getLocalizedModuleStoreData(moduleId, "zh-CN");
      const paragraphs = storeEntry?.aboutParagraphs ?? [];
      const characterCount = paragraphs.join("").replace(/\s/g, "").length;
      return { moduleId, paragraphCount: paragraphs.length, characterCount };
    })
    .filter((entry) => entry.paragraphCount < 2 || entry.characterCount < 120)
    .map((entry) => `${entry.moduleId}: ${entry.paragraphCount} paragraphs, ${entry.characterCount} chars`);

  assert.deepEqual(gaps, []);
});

test("all real zh-CN detail introduction paragraphs stay readable Chinese", () => {
  const gaps = realModuleIds()
    .flatMap((moduleId) => {
      const storeEntry = getLocalizedModuleStoreData(moduleId, "zh-CN");
      return (storeEntry?.aboutParagraphs ?? [])
        .map((paragraph, index) => ({ moduleId, index, chineseCharacters: (paragraph.match(/[\u3400-\u9fff]/g) ?? []).length }))
        .filter((entry) => entry.chineseCharacters < 8)
        .map((entry) => `${entry.moduleId}#${entry.index + 1}: ${entry.chineseCharacters} Chinese chars`);
    });

  assert.deepEqual(gaps, []);
});

test("zh-CN local detail stories are limited to official-source entries", () => {
  const leaks = Object.entries(ZH_CN_MODULE_STORE_COPY)
    .filter(([, copy]) => copy.storyParagraphs?.length)
    .filter(([moduleId]) => getModuleStoreData(moduleId)?.storeSource !== "official")
    .map(([moduleId]) => moduleId);

  assert.deepEqual(leaks, []);
  assert.ok(ZH_CN_MODULE_STORE_COPY.minecraft.storyParagraphs?.length >= 3);
});

test("zh-CN game detail introductions do not contain hosting workflow filler", () => {
  const polluted = realModuleIds()
    .flatMap((moduleId) => {
      const storeEntry = getLocalizedModuleStoreData(moduleId, "zh-CN");
      return (storeEntry?.aboutParagraphs ?? [])
        .filter((paragraph) => /开服前|运维资料|端口、密码、存档|手改安装目录|query port/i.test(paragraph))
        .map((paragraph) => `${moduleId}: ${paragraph}`);
    });

  assert.deepEqual(polluted, []);
});

test("mock API returns aggregate Steam review summaries without review bodies", async () => {
  const summary = await api.fetchSteamReviewSummary(1623730, "en-US");

  assert.equal(summary.app_id, 1623730);
  assert.equal(summary.review_score_desc, "Very Positive");
  assert.equal(summary.total_reviews, 10000);
  assert.equal(summary.positive_percent, 87);
  assert.equal(summary.source_url, "https://store.steampowered.com/app/1623730/#app_reviews_hash");
  assert.equal(Object.hasOwn(summary, "reviews"), false);
});

test("mock API localizes Steam review score descriptions for Chinese", async () => {
  const summary = await api.fetchSteamReviewSummary(1623730, "zh-CN");

  assert.equal(summary.review_score_desc, "特别好评");
  assert.equal(summary.positive_percent, 87);
});

test("mock API covers screenshot-only Steam detail pages with aggregate reviews", async () => {
  const summary = await api.fetchSteamReviewSummary(361420, "zh-CN");

  assert.equal(summary.app_id, 361420);
  assert.equal(summary.review_score_desc, "特别好评");
  assert.equal(summary.positive_percent, 92);
  assert.equal(summary.source_url, "https://store.steampowered.com/app/361420/#app_reviews_hash");
  assert.equal(Object.hasOwn(summary, "reviews"), false);
});

test("library detail page wires aggregate review summaries through the detail intelligence layer", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryDetailPage.tsx"), "utf8");

  assert.match(source, /fetchSteamReviewSummary/);
  assert.match(source, /LibraryReviewSummaryPanel/);
  assert.match(source, /formatSteamReviewSummary/);
  assert.match(source, /shouldFetchSteamReviewSummary/);
  assert.match(source, /<LibraryReviewSummaryPanel moduleId=\{props\.selected\.id\} storeEntry=\{props\.storeEntry\} \/>/);
});

test("library detail page caches Steam news and review requests before rendering digests", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryDetailPage.tsx"), "utf8");
  const updates = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryUpdatesPanel.tsx"), "utf8");

  assert.match(source, /createLibraryDetailResourceCache/);
  assert.match(source, /<LibraryUpdatesPanel storeEntry=\{props\.storeEntry\} \/>/);
  assert.match(updates, /formatSteamNewsDigest/);
  assert.match(updates, /steamNewsResourceCache\.read/);
  assert.match(source, /steamReviewSummaryResourceCache\.read/);
});

test("immersive detail layout remains a PC two-column workspace", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "styles", "library-cinematic.css"), "utf8");

  assert.match(source, /\.library-detail-page--immersive \.library-detail-steam-layout\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0,\s*1\.15fr\)\s+minmax\(280px,\s*340px\);/);
  assert.doesNotMatch(source, /@media\s*\(max-width:[\s\S]*?library-detail/);
});

test("detail header keeps a lightweight console hierarchy", () => {
  const pageSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryDetailPage.tsx"), "utf8");
  const styleSource = fs.readFileSync(path.join(desktopRoot, "src", "styles", "library-cinematic.css"), "utf8");

  assert.match(pageSource, /<ShellIcon name="chevron-left"/);
  assert.match(pageSource, /className="library-detail-header-meta"/);
  assert.match(pageSource, /className="library-detail-header-bg" aria-hidden="true"/);
  assert.match(pageSource, /library-detail-header-bg"[^>]*>\s*<div className="library-detail-header-gradient"/);
  assert.doesNotMatch(styleSource, /\.library-detail-header-bg \.module-cover/);
  assert.match(styleSource, /\.library-detail-header \.workspace-title\s*\{[\s\S]*?font-size:\s*var\(--text-page-title\)[\s\S]*?font-weight:\s*var\(--font-weight-semibold\)/);
  assert.match(styleSource, /\.library-detail-header \.workspace-header-actions\s*\{[\s\S]*?position:\s*absolute[\s\S]*?top:\s*18px[\s\S]*?right:\s*18px/);
  assert.match(styleSource, /\.library-detail-header \.library-back-button\s*\{[\s\S]*?height:\s*var\(--control-height-compact\)[\s\S]*?border-radius:\s*8px[\s\S]*?white-space:\s*nowrap/);
  assert.match(styleSource, /:root\[data-theme="light"\] \.library-experience--detail \.library-experience-backdrop\s*\{[\s\S]*?background:\s*var\(--theme-app-background\)/);
  assert.match(styleSource, /:root\[data-theme="light"\] \.library-detail-header-gradient\s*\{[\s\S]*?linear-gradient\(90deg/);
  assert.doesNotMatch(styleSource, /:root\[data-theme="light"\] \.library-detail-page--immersive::before/);
  assert.doesNotMatch(styleSource, /clamp\(3rem, 6vw, 5\.8rem\)/);
});

test("review summary labels are localized in English and Chinese", () => {
  const english = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-en-extra.ts"), "utf8");
  const chinese = fs.readFileSync(path.join(desktopRoot, "src", "i18n-messages-zh-ui.ts"), "utf8");
  const keys = [
    "library.detail.reviewsEyebrow",
    "library.detail.reviewsTitle",
    "library.detail.reviewsBody",
    "library.detail.reviewsPositive",
    "library.detail.reviewsTotal",
    "library.detail.reviewsLoading",
    "library.detail.reviewsUnavailable",
    "library.detail.reviewsError",
    "library.detail.reviewsOpen"
  ];

  for (const key of keys) {
    assert.match(english, new RegExp(`"${key}"`));
    assert.match(chinese, new RegExp(`"${key}"`));
  }
});

test("Tauri review command requests only aggregate Steam review summaries", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src-tauri", "src", "commands.rs"), "utf8");

  assert.match(source, /\("filter", "all"\)/);
  assert.match(source, /\("num_per_page", "0"\)/);
  assert.doesNotMatch(source, /\("filter", "summary"\)/);
});
