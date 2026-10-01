const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module, filename) {
  module._compile("", filename);
};
require.extensions[".png"] = function compileImageAsset(module, filename) {
  module._compile(`module.exports = ${JSON.stringify(`/fixture/${path.basename(filename)}`)};`, filename);
};

const rawStoreData = require(path.join(desktopRoot, "src", "data", "module-store-data.json"));
const {
  buildModuleMediaItems,
  getLocalizedModuleStoreData,
  getModuleStoreData,
  resolveStoreMediaUrl
} = require(path.join(desktopRoot, "src", "store-media.ts"));
const {
  resolveModuleCoverSrc,
  resolveModuleMediaFallbackSources
} = require(path.join(desktopRoot, "src", "module-art.ts"));

test("store media accepts only explicit HTTPS Steam and Xbox media hosts", () => {
  const steamImage = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/105600/header.jpg";
  const xboxImage = "https://store-images.s-microsoft.com/image/apps.example?q=90&w=1280&h=720";
  const xboxVideo = "https://cdn.trailers.xboxservices.com/trailers/example/master.m3u8";

  assert.equal(resolveStoreMediaUrl(steamImage), steamImage);
  assert.equal(resolveStoreMediaUrl(xboxImage), xboxImage);
  assert.equal(resolveStoreMediaUrl(xboxVideo), xboxVideo);
  assert.equal(resolveStoreMediaUrl("http://shared.akamai.steamstatic.com/example.jpg"), null);
  assert.equal(resolveStoreMediaUrl("https://example.com/example.jpg"), null);
  assert.equal(resolveStoreMediaUrl("https://user@shared.akamai.steamstatic.com/example.jpg"), null);
  assert.equal(resolveStoreMediaUrl("https://shared.akamai.steamstatic.com:444/example.jpg"), null);
  assert.equal(resolveStoreMediaUrl("/game-media/terraria/screenshot-1.jpg"), null);
  assert.equal(resolveStoreMediaUrl("data:image/png;base64,AA=="), null);
});

test("store metadata contains factual fields and remote media references only", () => {
  assert.ok(Object.keys(rawStoreData).length > 0);
  for (const [moduleId, entry] of Object.entries(rawStoreData)) {
    assert.equal(entry.shortDescription, "", `${moduleId} should not vendor Steam marketing copy`);
    assert.deepEqual(entry.aboutParagraphs, [], `${moduleId} should not vendor long Steam descriptions`);
    assert.doesNotMatch(JSON.stringify(entry), /game-(?:covers|media|assets)/, `${moduleId} should not reference local game media`);

    if (entry.storeSource === "steam") {
      assert.equal(resolveStoreMediaUrl(entry.coverUrl), entry.coverUrl, `${moduleId} should have an allowlisted cover`);
    }
    for (const screenshot of entry.screenshots) {
      assert.equal(resolveStoreMediaUrl(screenshot.sourceUrl), screenshot.sourceUrl);
      assert.equal(Object.hasOwn(screenshot, "path"), false);
    }
    for (const trailer of entry.trailers) {
      assert.equal(resolveStoreMediaUrl(trailer.streamUrl), trailer.streamUrl);
      if (trailer.posterUrl) {
        assert.equal(resolveStoreMediaUrl(trailer.posterUrl), trailer.posterUrl);
      }
      assert.equal(Object.hasOwn(trailer, "poster"), false);
    }
  }
});

test("Minecraft prioritizes Xbox artwork and retains the local cover as its final fallback", () => {
  const terraria = getModuleStoreData("terraria");
  const minecraft = getModuleStoreData("minecraft");

  assert.ok(terraria?.coverUrl);
  assert.equal(resolveModuleCoverSrc("terraria"), terraria.coverUrl);
  assert.match(minecraft?.coverUrl ?? "", /^https:\/\/store-images\.s-microsoft\.com\/image\//);
  assert.equal(resolveModuleCoverSrc("minecraft"), minecraft?.coverUrl);
  assert.match(
    resolveModuleMediaFallbackSources("minecraft", minecraft?.screenshots[1]?.src)[2] ?? "",
    /minecraft-cover\.png$/
  );
  assert.equal(resolveModuleCoverSrc("missing-module"), null);
});

test("Minecraft uses official Xbox media while keeping the local cover out of the remote media rail", () => {
  const translate = (key) => key;
  const minecraft = getLocalizedModuleStoreData("minecraft", "zh-CN");
  const mediaItems = buildModuleMediaItems("minecraft", minecraft, "zh-CN", translate);

  assert.equal(mediaItems.length, 5);
  assert.equal(mediaItems[0].kind, "trailer");
  assert.match(mediaItems[0].streamUrl ?? "", /^https:\/\/cdn\.trailers\.xboxservices\.com\//);
  assert.match(mediaItems[0].imageSrc ?? "", /^https:\/\/store-images\.s-microsoft\.com\/image\//);
  assert.deepEqual(mediaItems.slice(1).map((item) => item.kind), ["screenshot", "screenshot", "screenshot", "screenshot"]);
  assert.equal(mediaItems.some((item) => item.kind === "cover"), false);

  const terraria = getLocalizedModuleStoreData("terraria", "zh-CN");
  assert.equal(
    buildModuleMediaItems("terraria", terraria, "zh-CN", translate).some((item) => item.kind === "cover"),
    false
  );
});

test("media fallback candidates prefer the active asset, deduplicate the cover, and remain data driven", () => {
  const cover = resolveModuleCoverSrc("terraria");
  const screenshot = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/105600/ss_example.jpg";
  assert.ok(cover);
  const copies = (source) => [source,
    source.replace("shared.akamai.steamstatic.com", "shared.fastly.steamstatic.com"),
    source.replace("shared.akamai.steamstatic.com", "shared.cdn.steamchina.queniuam.com")];
  assert.deepEqual(resolveModuleMediaFallbackSources("terraria", screenshot), [...copies(screenshot), ...copies(cover)]);
  assert.deepEqual(resolveModuleMediaFallbackSources("terraria", cover), copies(cover));
  assert.deepEqual(resolveModuleMediaFallbackSources("missing-module", screenshot), copies(screenshot));
  assert.deepEqual(resolveModuleMediaFallbackSources("missing-module", null), []);
});

test("remote library image and video requests use a no-referrer document policy", () => {
  const indexHtml = fs.readFileSync(path.join(desktopRoot, "index.html"), "utf8");
  assert.match(indexHtml, /<meta name="referrer" content="no-referrer"\s*\/>/);

  for (const relativePath of [
    "src/components/ModuleCover.tsx",
    "src/views/library/LibraryAtmosphere.tsx"
  ]) {
    const source = fs.readFileSync(path.join(desktopRoot, relativePath), "utf8");
    assert.match(source, /referrerPolicy="no-referrer"/, `${relativePath} should make image policy explicit`);
  }
});

test("detail, thumbnail, poster, and atmosphere surfaces recover without exposing broken images", () => {
  const coverSource = fs.readFileSync(path.join(desktopRoot, "src", "components", "ModuleCover.tsx"), "utf8");
  const streamVideoSource = fs.readFileSync(path.join(desktopRoot, "src", "stream-video-player.ts"), "utf8");
  const detailSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryDetailPage.tsx"), "utf8");
  const atmosphereSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryAtmosphere.tsx"), "utf8");
  const fieldSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryAtmosphereField.tsx"), "utf8");
  const detailCssSource = fs.readFileSync(path.join(desktopRoot, "src", "styles", "core-pages", "library-detail.css"), "utf8");
  const baseCssSource = fs.readFileSync(path.join(desktopRoot, "src", "styles", "core-pages", "base.css"), "utf8");

  assert.match(coverSource, /resolveModuleMediaFallbackSources\(moduleId, previewFallbackImage \?\? imageSrc, variant\)/);
  assert.match(coverSource, /useMediaSource\(imageSources\)/);
  assert.match(coverSource, /onError=\{image\.onError\}/);
  assert.match(coverSource, /poster=\{displayImageSrc \?\? undefined\}/);
  assert.match(coverSource, /onError=\{\(\) => \{ setPreviewVideoReady\(false\); setPreviewVideoFailed\(true\); \}\}/);
  assert.match(streamVideoSource, /Hls\.Events\.ERROR[\s\S]*?data\.fatal && !disposed && hlsInstance === hls\) recoverHls\(\)/);
  assert.match(streamVideoSource, /const recoverHls = \(\) => \{[\s\S]*?if \(sourceGroups\.length < 2\) \{ fail\(\); return; \}[\s\S]*?disposeHls\(\);[\s\S]*?useNativeSource\(sourceGroups\[0\]\.length\)/);
  assert.match(streamVideoSource, /createOfficialHlsLoader\(Hls\.DefaultConfig\.loader, locale\)/);
  assert.doesNotMatch(streamVideoSource, /fetchSetup/);
  assert.doesNotMatch(streamVideoSource, /canPlayType\(/);
  assert.match(streamVideoSource, /const disposeHls = \(\) => \{[\s\S]*?hlsInstance = null;[\s\S]*?instance\.stopLoad\(\);[\s\S]*?finally \{[\s\S]*?instance\.destroy\(\)/);
  assert.match(streamVideoSource, /const fail = \(\) => \{[\s\S]*?disposeHls\(\);[\s\S]*?onUnavailable\(\)/);

  assert.match(detailSource, /const \[videoFallbackToCover, setVideoFallbackToCover\] = useState\(false\)/);
  assert.match(detailSource, /resolveModulePresentationFallbackCoverSrc\(moduleId\)/);
  assert.match(detailSource, /function LibraryTrailerPlayer[\s\S]*?<ModuleCover[\s\S]*?imageSrc=\{videoFallbackToCover \? fallbackCoverSrc : media\.imageSrc\}[\s\S]*?<StreamVideo/);
  assert.match(detailSource, /className=\{videoReady \? "library-media-video is-ready" : "library-media-video"\}/);
  assert.match(detailSource, /onError=\{\(\) => \{[\s\S]*?setVideoReady\(false\);[\s\S]*?setVideoFallbackToCover\(true\);[\s\S]*?\}\}/);
  assert.match(detailSource, /className="library-media-player library-media-resilient-cover"/);
  assert.match(detailSource, /className="library-media-steam-thumb-image"/);
  assert.doesNotMatch(detailSource, /library-media-steam-thumb-label/);
  assert.doesNotMatch(detailSource, /<img[\s\S]*?src=\{(?:props\.activeMedia\.imageSrc|item\.thumbnailSrc)\}/);
  assert.match(detailCssSource, /\.library-detail-stage-player\s*\{[\s\S]*?aspect-ratio:\s*16\s*\/\s*9/);
  assert.match(detailCssSource, /\.library-detail-stage-player \.module-cover--hero\s*\{[\s\S]*?aspect-ratio:\s*auto/);
  assert.match(
    detailCssSource,
    /\.library-detail-stage-player \.library-media-player,\s*\.library-detail-stage-player \.library-media-fallback\s*\{[\s\S]*?position:\s*absolute[\s\S]*?inset:\s*0/
  );
  assert.match(detailCssSource, /\.library-media-video\s*\{[\s\S]*?object-fit:\s*cover/);
  assert.match(detailCssSource, /\.library-media-video\s*\{[\s\S]*?opacity:\s*0/);
  assert.match(detailCssSource, /\.library-media-video\.is-ready\s*\{[\s\S]*?opacity:\s*1/);
  assert.match(detailCssSource, /\.library-media-steam-thumb \.module-cover\s*\{[\s\S]*?width:\s*100%[\s\S]*?height:\s*100%[\s\S]*?min-height:\s*0[\s\S]*?border:\s*0/);
  assert.match(baseCssSource, /\.module-cover-image\s*\{[\s\S]*?position:\s*absolute[\s\S]*?inset:\s*0[\s\S]*?width:\s*100%[\s\S]*?height:\s*100%[\s\S]*?object-fit:\s*cover/);

  assert.match(atmosphereSource, /resolveModuleMediaFallbackSources\(moduleId, imageSrc\)/);
  assert.match(atmosphereSource, /useMediaSource\(imageSources\)/);
  assert.match(atmosphereSource, /onError=\{image\.onError\}/);
  assert.match(fieldSource, /resolveModuleMediaFallbackSources\(props\.moduleId, props\.imageSrc\)/);
  assert.match(fieldSource, /loadMediaImage\(imageSources,[\s\S]*?cancelImage\(\)/);
  assert.match(fieldSource, /neutralTexture\.clone\(\)/);
});
