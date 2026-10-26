const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const catalog = require('../src/data/ark-map-artwork.json');
const { runBrowserFixture } = require('./helpers/runtime-browser-process.cjs');

// Normal regression runs use a controlled image boundary. An explicit evidence
// directory also renders the real downloaded Wiki bytes without redistributing them.
function artworkMiddleware(request, response, next) {
  if (request.url === '/__ark_artwork_mode') {
    response.setHeader('Content-Type', 'application/json');
    response.end(JSON.stringify({ real: Boolean(process.env.LANGAME_ARK_ARTWORK_EVIDENCE_DIR) })); return;
  }
  if (!request.url?.startsWith('/__ark_artwork/')) return next();
  const name = decodeURIComponent(request.url.slice('/__ark_artwork/'.length));
  const entry = catalog.maps.find(item => item.image === name);
  if (!entry) { response.writeHead(404).end(); return; }
  if (process.env.LANGAME_ARK_ARTWORK_EVIDENCE_DIR) {
    const file = path.join(process.env.LANGAME_ARK_ARTWORK_EVIDENCE_DIR, name);
    response.setHeader('Content-Type', 'image/jpeg'); response.end(fs.readFileSync(file));
  } else {
    response.setHeader('Content-Type', 'image/svg+xml');
    response.end(`<svg xmlns="http://www.w3.org/2000/svg" width="${entry.width}" height="${entry.height}"><rect width="100%" height="100%" fill="#395e65"/><text x="20" y="50" fill="white">Controlled image boundary</text></svg>`);
  }
}
for (const viewport of [{ width: 1280, height: 800 }, { width: 960, height: 600 }]) {
  test(`ARK artwork auto-selects edition and map at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const report = await runBrowserFixture({ fixturePath: 'ark-map-artwork-browser.html', viewport,
      fixtureMiddleware: artworkMiddleware, bundleWithMiddleware: true,
      screenshotPath: process.env.LANGAME_TOOLS_SCREENSHOT_DIR
        ? path.join(process.env.LANGAME_TOOLS_SCREENSHOT_DIR, `ark-auto-artwork-${viewport.width}x${viewport.height}.png`) : undefined });
    assert.equal(report.status, 'passed', report.error);
    assert.deepEqual(report.browser_errors, []);
    assert.ok(report.checks >= 14);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    console.log(`ARK_AUTO_ARTWORK_BROWSER ${JSON.stringify(report)}`);
  });
}
