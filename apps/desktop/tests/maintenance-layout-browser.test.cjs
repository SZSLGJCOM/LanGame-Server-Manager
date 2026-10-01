const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const preview = process.env.LANGAME_MAINTENANCE_PREVIEW;
assert.ok(!preview || ["backups", "save-policy", "runtime", "storage", "broadcast"].includes(preview),
  "LANGAME_MAINTENANCE_PREVIEW must name an existing maintenance category");

function previewMiddleware() {
  if (!preview) return undefined;
  const html = fs.readFileSync(path.join(__dirname, "helpers/maintenance-layout-browser.html"), "utf8");
  const openingTag = html.match(/<html\b[^>]*>/)?.[0];
  assert.ok(openingTag, "Maintenance fixture is missing its html element");
  const previewTag = openingTag.replace("<html", `<html data-maintenance-preview="${preview}"`);
  return (request, response, next) => {
    if (request.method !== "GET" || request.url?.split("?")[0] !== "/tests/helpers/maintenance-layout-browser.html") return next();
    // Amend the transformed HTML so Vite retains its React refresh preamble.
    const end = response.end;
    response.end = function (chunk, ...args) {
      const content = Buffer.isBuffer(chunk) ? chunk.toString("utf8") : chunk;
      if (typeof content !== "string") return end.call(this, chunk, ...args);
      const result = Buffer.from(content.replace(openingTag, previewTag), "utf8");
      if (!this.headersSent) this.setHeader("Content-Length", result.length);
      return end.call(this, result, ...args);
    };
    next();
  };
}

for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`maintenance categories preserve operations and drafts at ${viewport.width} x ${viewport.height}`, { timeout: 90000 }, async () => {
    const screenshot = process.env.LANGAME_MAINTENANCE_SCREENSHOT;
    const parsed = screenshot ? path.parse(screenshot) : null;
    const screenshotPath = viewport.width === 960 && parsed
      ? path.join(parsed.dir, `${parsed.name}-compact${parsed.ext || ".png"}`) : screenshot;
    const report = await runBrowserFixture({ fixturePath: "maintenance-layout-browser.html", keyboard: true,
      viewport, screenshotPath, fixtureMiddleware: previewMiddleware() });
    console.log(`MAINTENANCE_LAYOUT_BROWSER ${JSON.stringify(report)}`);
    assert.equal(report.status, "passed");
    assert.equal(report.control_measurements.length, 17);
    assert.deepEqual(report.control_violations, []);
    assert.equal(report.checks, 18);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
