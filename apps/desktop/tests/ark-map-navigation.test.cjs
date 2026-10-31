const assert = require('node:assert/strict');
const fs = require('node:fs');
const test = require('node:test');
const { transpileTypeScript } = require('../scripts/typescript_source_tools.cjs');
require.extensions['.ts'] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, 'utf8'), filename), filename);
const { clampArkMapView } = require('../src/ark-map-view.ts');

test('ARK minimum zoom centers the full map in square, wide and tall viewports', () => {
  for (const [width, height] of [[600, 600], [900, 600], [600, 900]]) {
    for (const [x, y] of [[-2000, -2000], [2000, 2000], [100, 900]]) {
      assert.deepEqual(clampArkMapView({ zoom: 1, x, y }, { width, height }), { zoom: 1, x: 500, y: 500 });
    }
  }
});

test('ARK panning accounts for the complete rectangular SVG viewport at either edge', () => {
  const viewport = { width: 900, height: 600 };
  assert.deepEqual(clampArkMapView({ zoom: 2, x: -2000, y: -2000 }, viewport), { zoom: 2, x: 375, y: 250 });
  assert.deepEqual(clampArkMapView({ zoom: 2, x: 2000, y: 2000 }, viewport), { zoom: 2, x: 625, y: 750 });
  assert.deepEqual(clampArkMapView({ zoom: 2, x: 2000, y: -2000 }, { width: 600, height: 900 }), { zoom: 2, x: 750, y: 375 });
  assert.deepEqual(clampArkMapView({ zoom: 2, x: 900, y: 900 }, { width: 1200, height: 600 }), { zoom: 2, x: 500, y: 750 });
});

test('ARK resizing and zooming out reclamp the stored center without preventing valid panning', () => {
  const viewport = { width: 600, height: 600 }, panned = { zoom: 2, x: 650, y: 700 };
  assert.strictEqual(clampArkMapView(panned, viewport), panned);
  const resized = clampArkMapView(panned, { width: 1200, height: 600 });
  assert.deepEqual(resized, { zoom: 2, x: 500, y: 700 });
  assert.deepEqual(clampArkMapView(resized, viewport), resized, 'restoring the pane size retains the corrected center');
  assert.deepEqual(clampArkMapView({ ...panned, zoom: .5 }, viewport), { zoom: 1, x: 500, y: 500 });
  assert.deepEqual(clampArkMapView({ zoom: 20, x: 500, y: 500 }, viewport), { zoom: 12, x: 500, y: 500 });
});
