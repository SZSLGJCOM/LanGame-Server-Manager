const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const { shouldUseLocalLibraryStory } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "library",
  "library-story-source.ts"
));

function steamEntry(overrides = {}) {
  return {
    storeSource: "steam",
    storeAppId: 252490,
    storeName: "Steam Survival Sandbox",
    shortDescription: "程序化太空沙盒。",
    aboutParagraphs: ["这是一段本地维护的中文游戏详情。"],
    genres: [],
    categories: [],
    developers: [],
    publishers: [],
    releaseDate: "",
    storeUrl: "",
    officialLinks: [],
    screenshots: [],
    trailers: [],
    ...overrides
  };
}

test("zh-CN Steam detail pages keep remote Steam HTML as the primary story source", () => {
  assert.equal(shouldUseLocalLibraryStory("zh-CN", steamEntry()), false);
  assert.equal(shouldUseLocalLibraryStory("en-US", steamEntry()), false);
  assert.equal(
    shouldUseLocalLibraryStory("zh-CN", steamEntry({ aboutParagraphs: ["English-only Steam fallback."] })),
    false
  );
});

test("official-source zh-CN detail pages can use local Chinese story copy", () => {
  assert.equal(
    shouldUseLocalLibraryStory(
      "zh-CN",
      steamEntry({
        storeSource: "official",
        storeAppId: null
      })
    ),
    true
  );
});

test("Steam story panel keeps local paragraphs as a fetch fallback only", () => {
  const source = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "LibraryStoryPanel.tsx"), "utf8");

  assert.match(source, /library-story-panel--steam-fallback/);
  assert.match(source, /if \(html\)[\s\S]*?library-story-panel--steam-html/);
  assert.match(source, /storyLocalFallback/);
});

test("Steam story media drops remote styles, validates size hints, and keeps natural aspect ratio", () => {
  const pageSource = fs.readFileSync(path.join(desktopRoot, "src", "views", "library", "steam-about-html.ts"), "utf8");
  const cssSource = fs.readFileSync(path.join(desktopRoot, "src", "styles", "core-pages", "refinements.css"), "utf8");
  const imageRule = /\.library-story-html img,\s*\.library-story-html video\s*\{(?<body>[^}]*)\}/.exec(cssSource)?.groups?.body ?? "";

  const cases = [
    ["img", "640", "360", true],
    ["video", "99999", "1", true],
    ["img", "100%", "-1", false],
    ["video", "100000", "0", false],
    ["img", "01", "1.5", false],
    ["div", "640", "360", false]
  ];
  const elements = cases.map(([tagName, width, height]) => {
    const attributes = new Map(Object.entries({ width, height, style: "width: 100%; height: 1px" }));
    return { tagName, values: attributes,
      get attributes() { return Array.from(attributes, ([name, value]) => ({ name, value })); },
      removeAttribute(name) { attributes.delete(name); }, setAttribute(name, value) { attributes.set(name, value); }
    };
  });
  const exports = {};
  vm.runInNewContext(transpileTypeScript(pageSource, "steam-about-html.ts"), {
    exports,
    DOMParser: class {
      parseFromString() {
        return { body: { innerHTML: "", querySelectorAll: (selector) => selector === "*" ? elements : [] } };
      }
    }
  });
  exports.sanitizeSteamAboutHtml("fixture");
  for (const [index, element] of elements.entries()) {
    assert.equal(element.values.has("style"), false, "remote inline styles must not override local layout");
    assert.equal(element.values.has("width"), cases[index][3], `${cases[index]} width`);
    assert.equal(element.values.has("height"), cases[index][3], `${cases[index]} height`);
  }

  assert.match(imageRule, /width:\s*auto;/);
  assert.match(imageRule, /max-width:\s*min\(var\(--library-steam-media-max-width\),\s*100%\);/);
  assert.match(imageRule, /height:\s*auto;/);
  assert.match(imageRule, /object-fit:\s*contain;/);
  assert.doesNotMatch(imageRule, /(?:^|\n)\s*width:\s*100%;/);
});
