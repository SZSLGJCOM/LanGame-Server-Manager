const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { DST_MAP_MAX_VIEW_ZOOM, clampDstMapCenter, dstMapZoomLimit, clampDstMapZoom, stepDstMapZoom, anchoredDstMapCenter } = require("../src/dst-map-view.ts");
const { dstLandmarkIconSize, indexDstLandmarks, visibleDstLandmarks } = require("../src/dst-map-landmarks.ts");

function landmark(id, prefab, mapX = 50, mapZ = 50, priority = 0) {
  return { id, prefab, icon: `${prefab}.png`, mapX, mapZ, priority };
}

test("a large world reaches an inspectable tile scale while icons remain bounded", () => {
  for (const [width, height, viewportWidth, viewportHeight] of [[4096, 4096, 1280, 320], [1024, 600, 960, 600], [425, 425, 1280, 720]]) {
    const fit = Math.max(viewportWidth / width, viewportHeight / height);
    const maximum = dstMapZoomLimit(width, height, viewportWidth, viewportHeight);
    assert.ok(maximum <= DST_MAP_MAX_VIEW_ZOOM);
    assert.ok(maximum * fit >= 24, "full close-up must expose at least 24 CSS pixels per map tile");
    assert.equal(dstLandmarkIconSize(maximum * fit), 24, "zoom cannot enlarge a landmark over terrain indefinitely");
    assert.equal(clampDstMapZoom(maximum * 2, maximum), maximum);
    assert.equal(stepDstMapZoom(maximum, 1, maximum), maximum);
  }
});

test("zoom limits preserve existing defaults and reject invalid dimensions", () => {
  assert.equal(clampDstMapZoom(100), 10);
  for (const dimensions of [[0, 5, 10, 10], [10, Infinity, 10, 10], [10, 10, NaN, 10]]) assert.equal(dstMapZoomLimit(...dimensions), 10);
  assert.equal(clampDstMapZoom(NaN, .5), 1);
  assert.equal(clampDstMapZoom(.5), 1, "zoom never shrinks the world below the whole-world fit");
  assert.equal(dstMapZoomLimit(4096, 4096, .001, .001), DST_MAP_MAX_VIEW_ZOOM);
  const center = { x: 100, y: 90 }, offset = { x: 48, y: -30 };
  const next = anchoredDstMapCenter(center, offset, .5, 24);
  assert.deepEqual({ x: next.x + offset.x / 24, y: next.y + offset.y / 24 },
    { x: center.x + offset.x / .5, y: center.y + offset.y / .5 });
});

test("panning stops at the world border and an axis showing the whole world cannot pan", () => {
  const world = { width: 100, height: 80 }, viewport = { width: 600, height: 300 };
  // Scale 6: the full width is visible, so X is pinned and Y may travel 25 units each way.
  assert.deepEqual(clampDstMapCenter({ x: -500, y: -500 }, world, viewport, 6), { x: 50, y: 25 });
  assert.deepEqual(clampDstMapCenter({ x: 500, y: 500 }, world, viewport, 6), { x: 50, y: 55 });
  assert.deepEqual(clampDstMapCenter({ x: 50, y: 40 }, world, viewport, 6), { x: 50, y: 40 });
  assert.deepEqual(clampDstMapCenter({ x: 0, y: 80 }, world, viewport, 24), { x: 12.5, y: 73.75 });
  assert.deepEqual(clampDstMapCenter({ x: NaN, y: 10 }, world, viewport, 24), { x: 50, y: 10 });
  assert.deepEqual(clampDstMapCenter({ x: 3, y: 3 }, world, { width: 0, height: 0 }, 0), { x: 3, y: 3 });
  // A square world fitted into a square pane: 100% shows everything, so every drag target collapses to the middle.
  assert.deepEqual(clampDstMapCenter({ x: 0, y: 400 }, { width: 400, height: 400 }, { width: 600, height: 600 }, 1.5), { x: 200, y: 200 });
});

test("decluttering keeps the higher-priority entity without changing native entity data", () => {
  const native = landmark("native", "portal", 50, 50, 100);
  const hidden = landmark("hidden", "unknown_mod_machine", 50, 50, -10);
  const items = [native, hidden], before = JSON.stringify(items), index = indexDstLandmarks(items);
  const view = { centerX: 50, centerZ: 50, width: 300, height: 200, scale: 2 };
  assert.deepEqual(visibleDstLandmarks(index, view).map(item => item.id), ["native"]);
  assert.equal(JSON.stringify(items), before);
});
