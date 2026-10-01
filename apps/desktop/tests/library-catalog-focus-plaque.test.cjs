const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const desktopRoot = path.resolve(__dirname, "..");

function readDesktopSource(relativePath) {
  return fs.readFileSync(path.join(desktopRoot, ...relativePath.split("/")), "utf8");
}

function sourceSection(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  assert.notEqual(start, -1, `${startMarker} is missing`);
  const end = source.indexOf(endMarker, start + startMarker.length);
  assert.notEqual(end, -1, `${endMarker} is missing after ${startMarker}`);
  return source.slice(start, end);
}

function cssAtRuleBlocks(source, atRule) {
  const blocks = [];
  let offset = 0;

  while (offset < source.length) {
    const start = source.indexOf(atRule, offset);
    if (start < 0) {
      break;
    }
    const openingBrace = source.indexOf("{", start);
    assert.notEqual(openingBrace, -1, `${atRule} has no opening brace`);
    let depth = 1;
    let cursor = openingBrace + 1;
    while (cursor < source.length && depth > 0) {
      if (source[cursor] === "{") {
        depth += 1;
      } else if (source[cursor] === "}") {
        depth -= 1;
      }
      cursor += 1;
    }
    assert.equal(depth, 0, `${atRule} has no closing brace`);
    blocks.push(source.slice(start, cursor));
    offset = cursor;
  }

  return blocks;
}

const catalogPageSource = readDesktopSource("src/views/library/LibraryCatalogPage.tsx");
const cinematicCssSource = readDesktopSource("src/styles/library-cinematic.css");
const ambientCssSource = readDesktopSource("src/styles/library-ambient.css");
const libraryViewSource = readDesktopSource("src/views/LibraryView.tsx");
const atmosphereSource = readDesktopSource("src/views/library/LibraryAtmosphere.tsx");
const atmosphereFieldSource = readDesktopSource("src/views/library/LibraryAtmosphereField.tsx");
const webGlCapabilitiesSource = readDesktopSource("src/webgl-capabilities.ts");
const moduleCoverSource = readDesktopSource("src/components/ModuleCover.tsx");
const tileSource = sourceSection(catalogPageSource, "const LibraryCatalogTile", "function buildTileModels");

test("pointer click selects a card while pointer confirmation waits for the card to settle", () => {
  const clickBinding = tileSource.match(/onClick=\{([^\n]+)\}/)?.[1] ?? "";
  const doubleClickBinding = tileSource.match(/onDoubleClick=\{([^\n]+)\}/)?.[1] ?? "";

  assert.match(clickBinding, /props\.onSelect\(target\)/);
  assert.doesNotMatch(clickBinding, /onConfirm|onOpen/);
  assert.match(doubleClickBinding, /handleDoubleClick/);
  assert.match(tileSource, /canConfirmLibraryCatalogPointerSelection\(props\.active, activeSinceRef\.current, Date\.now\(\)\)/);
  assert.match(tileSource, /props\.onConfirm\(target\)/);
});

test("the complete library uses one horizontal focus rail", () => {
  assert.match(catalogPageSource, /const railClassName =/);
  assert.match(catalogPageSource, /className=\{railClassName\}[\s\S]*?role="listbox"/);
  assert.doesNotMatch(catalogPageSource, /library-catalog-recent-rail|library-catalog-grid/);
  assert.doesNotMatch(catalogPageSource, /recentModules|data-library-surface/);
  assert.match(tileSource, /id=\{libraryCatalogOptionId\(target\)\}/);
  assert.match(tileSource, /aria-selected=\{props\.active\}/);
  assert.match(catalogPageSource, /tabIndex=\{0\}/);
  assert.match(catalogPageSource, /onKeyDown=\{props\.onRailKeyDown\}/);
  assert.match(catalogPageSource, /onPage\("left"\)/);
  assert.match(catalogPageSource, /onPage\("right"\)/);
});

test("context menu owns navigation until it is confirmed or dismissed", () => {
  assert.match(libraryViewSource, /if \(contextMenu\) \{[\s\S]*?action\.type === "move"[\s\S]*?moveContextMenuFocus/);
  assert.match(libraryViewSource, /action\.type === "confirm"[\s\S]*?confirmContextMenuAction/);
  assert.match(catalogPageSource, /function handleContextMenuKeyDown/);
  assert.match(catalogPageSource, /props\.onCloseContext\(false\)/);
});

test("the catalog ships with the library route without a redundant loading page", () => {
  assert.match(libraryViewSource, /import \{ LibraryCatalogPage \} from "\.\/library\/LibraryCatalogPage"/);
  assert.doesNotMatch(libraryViewSource, /const LibraryCatalogPage = lazy/);
  assert.doesNotMatch(libraryViewSource, /library\.loadingCatalog|LibraryViewFallback/);
  assert.match(libraryViewSource, /const LibraryDetailPage = lazy/);
});

test("decorative WebGL loads behind the catalog instead of blocking it", () => {
  assert.match(libraryViewSource, /const LibraryAtmosphereField = lazy/);
  assert.match(libraryViewSource, /<Suspense fallback=\{null\}>[\s\S]{0,800}?<LibraryAtmosphereField/);
  assert.match(libraryViewSource, /<LibraryAtmosphere[\s\S]*?<Suspense fallback=\{null\}>/);
});

test("the remaining detail loading state fills the library surface", () => {
  assert.match(libraryViewSource, /className="library-view-loading"[\s\S]{0,200}?role="status"/);
  assert.match(libraryViewSource, /<Suspense fallback=\{<LibraryDetailLoadingState \/>\}>/);
  assert.match(cinematicCssSource, /\.library-view-loading\s*\{[\s\S]{0,500}?width:\s*100%[\s\S]{0,500}?place-content:\s*center/);
});

test("horizontal rail wheel input is continuous, non-passive and releases at its boundaries", () => {
  assert.match(catalogPageSource, /addEventListener\("wheel", handleWheel, \{ passive: false \}\)/);
  assert.doesNotMatch(catalogPageSource, /onWheel=\{/);
  assert.match(libraryViewSource, /const canContinue = delta < 0/);
  assert.match(libraryViewSource, /if \(!canContinue\) \{\s*return;/);
  assert.doesNotMatch(libraryViewSource, /resolveLibraryCatalogWheelNavigation|wheelAccumulator/);
  assert.doesNotMatch(cinematicCssSource, /scroll-behavior:\s*smooth/);
  assert.doesNotMatch(cinematicCssSource, /scroll-snap-type:\s*x/);
});

test("focus travel is short, cancelable and independent from browser smooth scrolling", () => {
  assert.match(libraryViewSource, /resolveLibraryCatalogAlignmentScrollLeft/);
  assert.match(libraryViewSource, /const cancelRailMotion = useCallback/);
  assert.match(libraryViewSource, /window\.cancelAnimationFrame/);
  assert.match(libraryViewSource, /CATALOG_RAIL_MOTION_MIN_MS\s*=\s*140/);
  assert.match(libraryViewSource, /CATALOG_RAIL_MOTION_MAX_MS\s*=\s*210/);
  assert.match(libraryViewSource, /prefers-reduced-motion: reduce/);
  assert.doesNotMatch(libraryViewSource, /scrollIntoView|behavior:\s*"smooth"/);
});

test("catalog exposes search, portrait selection and an explicit details action", () => {
  assert.match(catalogPageSource, /library-catalog-search-shell/);
  assert.match(catalogPageSource, /library-catalog-rail-shell/);
  assert.match(catalogPageSource, /library-catalog-focus-copy/);
  assert.match(catalogPageSource, /library-catalog-focus-plaque/);
  assert.match(catalogPageSource, /library-catalog-toolbar/);
  assert.match(catalogPageSource, /className="primary-button library-catalog-open-details"[\s\S]*?onClick=\{\(\) => props\.onConfirm\(\{ moduleId: focusedModule\.id \}\)\}/);
  assert.doesNotMatch(catalogPageSource, /onFilterChange|onSortChange|onToggleFavorite/);
});

test("catalog focus uses the raised original card and detached title plaque without image clones", () => {
  assert.doesNotMatch(catalogPageSource, /library-catalog-active-frame|library-catalog-active-card-motion/);
  assert.match(tileSource, /props\.active \? "library-catalog-tile is-active"/);
  assert.match(cinematicCssSource, /\.library-home-page--cinematic \.library-catalog-tile\.is-active/);
  assert.match(cinematicCssSource, /\.library-home-page--cinematic \.library-catalog-focus-plaque/);
  assert.match(cinematicCssSource, /\.library-catalog-tile\.is-active\s*\{[\s\S]*?scale\(1\.1\)/);
  assert.match(cinematicCssSource, /\.library-catalog-tile::before[\s\S]*?\.library-catalog-tile\.is-active::before/);
  assert.match(cinematicCssSource, /\.library-catalog-tile::before\s*\{[^}]*border:\s*2px solid var\(--shell-focus\)/);
  assert.match(cinematicCssSource, /\.library-catalog-focus-plaque\s*\{[\s\S]*?min-height:\s*60px[\s\S]*?border:\s*0[\s\S]*?background:\s*transparent/);
  assert.match(cinematicCssSource, /\.library-catalog-focus-status\s*\{[\s\S]*?border:\s*0[\s\S]*?background:\s*transparent/);
  assert.doesNotMatch(catalogPageSource, /library-catalog-focus-rail/);
});

test("the persistent atmosphere follows settled preview focus without changing the catalog hierarchy", () => {
  assert.match(libraryViewSource, /CATALOG_ATMOSPHERE_PREVIEW_DELAY_MS\s*=\s*140/);
  assert.match(libraryViewSource, /catalogPreviewModuleId \?\? effectiveTarget\?\.moduleId/);
  assert.match(libraryViewSource, /setCatalogAtmosphereModuleId\(nextId\)/);
  assert.match(catalogPageSource, /onPreview=\{props\.onPreviewModuleChange\}/);
  assert.doesNotMatch(catalogPageSource, /LibraryAtmosphere|LibraryAtmosphereField|<Canvas/);
  assert.match(libraryViewSource, /<LibraryAtmosphere[\s\S]*?<LibraryAtmosphereField/);
  assert.doesNotMatch(libraryViewSource, /<LibraryAtmosphereField[\s\S]*?key=/);
  assert.match(catalogPageSource, /<div key=\{focusedModule\.id\} className="library-catalog-focus-plaque">/);
  assert.match(cinematicCssSource, /@keyframes libraryCatalogBackgroundIn/);
  assert.match(cinematicCssSource, /@keyframes libraryCatalogPlaqueIn/);
  assert.match(cinematicCssSource, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.library-atmosphere-image/);
});

test("the library atmosphere is one WebGL2 fullscreen plane with bounded demand rendering", () => {
  assert.equal((atmosphereFieldSource.match(/<Canvas/g) ?? []).length, 1);
  assert.equal((atmosphereFieldSource.match(/<mesh/g) ?? []).length, 1);
  assert.match(atmosphereFieldSource, /supportsWebGl2/);
  assert.match(webGlCapabilitiesSource, /getContext\("webgl2"/);
  assert.match(webGlCapabilitiesSource, /WEBGL_lose_context/);
  assert.match(atmosphereFieldSource, /glslVersion=\{THREE\.GLSL3\}/);
  assert.match(atmosphereFieldSource, /dpr=\{\[1, 1\.25\]\}/);
  assert.match(atmosphereFieldSource, /frameloop="demand"/);
  assert.match(atmosphereFieldSource, /antialias:\s*false/);
  assert.match(atmosphereFieldSource, /depth:\s*false/);
  assert.match(atmosphereFieldSource, /stencil:\s*false/);
  assert.match(atmosphereFieldSource, /<planeGeometry args=\{\[2, 2\]\}/);
  assert.doesNotMatch(atmosphereFieldSource, /EffectComposer|Bloom|<points|StreamVideo/);
  assert.doesNotMatch(atmosphereFieldSource, /lensRing|radius \* 8\.0|fieldPoint\.x \+ fieldPoint\.y/);
});

test("texture transitions retain only current and next images and dispose stale loads", () => {
  assert.match(atmosphereFieldSource, /uCurrentTexture/);
  assert.match(atmosphereFieldSource, /uNextTexture/);
  assert.match(atmosphereFieldSource, /generation !== generationRef\.current/);
  assert.match(atmosphereFieldSource, /loaded\.texture\.dispose\(\)/);
  assert.match(atmosphereFieldSource, /currentTextureRef\.current\?\.texture\.dispose\(\)/);
  assert.match(atmosphereFieldSource, /nextTextureRef\.current\?\.texture\.dispose\(\)/);
  assert.match(atmosphereFieldSource, /texture\.colorSpace = THREE\.SRGBColorSpace/);
  assert.match(atmosphereFieldSource, /texture\.generateMipmaps = false/);
  assert.doesNotMatch(atmosphereSource, /StreamVideo|videoSrc|preferVideo/);
});

test("WebGL2 is progressive enhancement and cannot intercept catalog interaction", () => {
  assert.match(libraryViewSource, /useMotionValue\(0\)/);
  assert.match(libraryViewSource, /onPointerMove=\{handleAtmospherePointerMove\}/);
  assert.match(atmosphereFieldSource, /useReducedMotion\(\)/);
  assert.match(atmosphereFieldSource, /visibilitychange/);
  assert.match(atmosphereFieldSource, /if \(!supported\) \{\s*return null;/);
  assert.match(atmosphereSource, /library-atmosphere-image/);
  assert.match(cinematicCssSource, /\.library-atmosphere-field\s*\{[\s\S]*?pointer-events:\s*none/);
  assert.match(cinematicCssSource, /\.library-atmosphere-field canvas\s*\{[\s\S]*?pointer-events:\s*none/);
  assert.doesNotMatch(ambientCssSource, /library-home-stage-background/);
});

test("the settled card restores muted trailer preview without animating in reduced-motion mode", () => {
  assert.match(catalogPageSource, /item\.kind === "trailer" && item\.streamUrl/);
  assert.match(tileSource, /previewVideoUrl=\{props\.previewVideoUrl\}/);
  assert.match(tileSource, /previewVideoPoster=\{props\.previewVideoPoster\}/);
  assert.match(tileSource, /previewVideoActive=\{props\.active\}/);
  assert.match(moduleCoverSource, /autoPlay\s+muted\s+loop/);
  assert.match(moduleCoverSource, /if \(motionQuery\.matches\) \{\s*return;/);
});

test("the focused game name uses a calm status, title and subtitle hierarchy", () => {
  assert.match(catalogPageSource, /library-catalog-focus-status is-installed/);
  assert.match(catalogPageSource, /library-catalog-focus-status-icon/);
  assert.doesNotMatch(catalogPageSource, /library-catalog-focus-kicker/);
  assert.match(cinematicCssSource, /\.library-home-page--cinematic \.library-catalog-focus-status/);
  assert.match(cinematicCssSource, /\.library-home-page--cinematic \.library-catalog-focus-title[\s\S]*?letter-spacing:\s*-0\.03em/);
});

test("the rail contains the outer selection ring throughout its lift and scale animation", () => {
  const rail = cinematicCssSource.match(/\.library-home-page--cinematic \.library-catalog-rail\s*\{([^}]+)\}/)?.[1];
  const tile = sourceSection(cinematicCssSource, ".library-home-page--cinematic .library-catalog-tile {", ".library-home-page--cinematic .library-catalog-focus-copy {");
  const width = Number(tile.match(/width:\s*([\d.]+)px/)?.[1]);
  const ratio = Number(tile.match(/aspect-ratio:\s*([\d.]+)/)?.[1]);
  const ringOutset = -Number(tile.match(/inset:\s*(-?[\d.]+)px/)?.[1]);
  const padding = rail?.match(/padding:\s*([\d.]+)px\s+var\([^)]*\)(?:\s+([\d.]+)px)?/);
  assert.ok(padding, "The rail must reserve vertical space around its cards");
  const topPadding = Number(padding[1]);
  const bottomPadding = Number(padding[2] ?? padding[1]);
  const transforms = [...tile.matchAll(/translate3d\(0,\s*(-?[\d.]+)px,\s*0\)\s*scale\(([\d.]+)\)/g)];
  assert.ok(transforms.length > 0, "Selection movement must be included in the clipping check");
  for (const [, offset, scale] of transforms) {
    const expansion = (width / ratio) * (Number(scale) - 1) / 2 + ringOutset * Number(scale);
    assert.ok(topPadding >= expansion - Number(offset), `The top ring clips at translateY(${offset}px) scale(${scale})`);
    assert.ok(bottomPadding >= expansion + Number(offset), `The bottom ring clips at translateY(${offset}px) scale(${scale})`);
  }
});

test("catalog restores portrait cover art and keeps its layout PC scoped", () => {
  assert.match(cinematicCssSource, /aspect-ratio:\s*0\.76/);
  assert.match(cinematicCssSource, /\.library-home-page--cinematic \.library-catalog-rail\s*\{[\s\S]*?display:\s*flex/);
  assert.match(cinematicCssSource, /flex:\s*0 0 196px/);
  assert.match(cinematicCssSource, /\.shell-content-scroll:has\(\.library-experience--catalog\)[\s\S]*?overflow-y:\s*hidden/);
  assert.match(cinematicCssSource, /\.shell-content-body:has\(\.library-experience--catalog\) > \.library-experience--catalog/);
  assert.doesNotMatch(cinematicCssSource, /library-catalog-grid|library-catalog-recent-rail/);

  const narrowViewportRules = cssAtRuleBlocks(cinematicCssSource, "@media")
    .filter((block) => /max-width\s*:/i.test(block));
  for (const block of narrowViewportRules) {
    assert.doesNotMatch(block, /library-catalog/);
  }
  assert.doesNotMatch(cinematicCssSource, /@media\s*\(max-width:[\s\S]*?library-detail/);
  assert.match(cinematicCssSource, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.library-catalog-tile\.is-active[\s\S]*?transform:\s*none/);
});

test("only focused horizontal neighbors decode eagerly", () => {
  assert.match(catalogPageSource, /COVER_DECODE_RADIUS\s*=\s*2/);
  assert.match(catalogPageSource, /function resolveEagerIds\(/);
  assert.match(catalogPageSource, /eager=\{eagerIds\.has\(tile\.module\.id\)\}/);
  assert.match(tileSource, /imageLoading=\{props\.eager \? "eager" : "lazy"\}/);
  assert.match(moduleCoverSource, /decoding="async"/);
  assert.doesNotMatch(catalogPageSource, /new Image\(|image\.decode|resolveModuleCoverSrc/);
  assert.doesNotMatch(catalogPageSource, /window\.fetch\(|force-cache/);
  assert.doesNotMatch(catalogPageSource, /catalogCoverPreloadRef/);
  assert.doesNotMatch(cinematicCssSource, /will-change\s*:/);
});
