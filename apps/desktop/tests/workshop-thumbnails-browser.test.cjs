const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

function previewImage(request, response, next) {
  const shape = request.url?.match(/^\/__workshop_preview\/(wide|tall|square)\.svg$/)?.[1];
  if (!shape) return next();
  const [width, height, color] = shape === "wide" ? [640, 320, "#b55524"]
    : shape === "tall" ? [320, 640, "#246e9b"] : [320, 320, "#437c36"];
  response.setHeader("Content-Type", "image/svg+xml");
  response.end(`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><rect width="100%" height="100%" fill="${color}"/><circle cx="${width / 2}" cy="${height / 2}" r="80" fill="#eee8d7"/><text x="50%" y="52%" text-anchor="middle" font-family="sans-serif" font-size="30" fill="#17222a">${shape}</text></svg>`);
}

for (const viewport of [{ width: 1366, height: 768 }, { width: 1920, height: 1080 }]) {
  test(`Workshop thumbnails stay square across games and image shapes at ${viewport.width}x${viewport.height}`,
    { timeout: 90000 }, async () => {
      const output = process.env.LANGAME_WORKSHOP_THUMBNAILS_OUTPUT;
      if (output) {
        assert.ok(path.isAbsolute(output));
        const relative = path.relative(path.resolve(__dirname, "../../.."), output);
        assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Evidence stays outside the repository");
        await fs.mkdir(output, { recursive: true });
      }
      const name = `${viewport.width}x${viewport.height}`;
      const report = await runBrowserFixture({ fixturePath: "workshop-thumbnails-browser.html", viewport,
        fixtureMiddleware: previewImage, bundleWithMiddleware: true,
        screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
      if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
      console.log(`WORKSHOP_THUMBNAILS ${JSON.stringify({ viewport: report.viewport,
        checks: report.checks?.length, failures: report.failures, measurements: report.measurements?.length,
        thumbnail_sizes: [...new Set((report.measurements ?? []).map((entry) => `${entry.width}x${entry.height}`))],
        browser_errors: report.browser_errors })}`);
      assert.equal(report.status, "passed", report.error);
      assert.deepEqual(report.failures, [], "Rendered thumbnail bounds must stay square");
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_exited, true);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
    });
}
