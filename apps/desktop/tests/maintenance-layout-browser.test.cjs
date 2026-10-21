const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const preview = process.env.LANGAME_MAINTENANCE_PREVIEW;
assert.ok(!preview || ["saves", "saves:autosave", "saves:import", "backups", "backups:policy",
  "runtime", "runtime:resources", "runtime:recovery", "runtime:daily", "runtime:program", "broadcast"].includes(preview),
  "LANGAME_MAINTENANCE_PREVIEW must name an existing maintenance category, optionally with one of its popovers");

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
    assert.equal(report.control_measurements.length, 14);
    assert.deepEqual(report.control_violations, []);
    assert.equal(report.checks, 24);
    assert.equal(report.broadcast_send_count, 3, "Pending confirmation must never automatically resend a broadcast");
    assert.deepEqual(report.broadcast_history_statuses, ["待确认", "已发送", "待确认", "失败"]);
    assert.equal(report.broadcast_history_instance_isolated, true);
    // The stop-backup toggle and retention input share one control row (or one column when stacked).
    const row = report.policy_alignment;
    assert.ok(row, "maintenance fixture must measure the backup policy row");
    if (row.columns === 2) {
      assert.ok(Math.abs(row.toggle_top - row.input_top) <= 1 && Math.abs(row.toggle_bottom - row.input_bottom) <= 1, JSON.stringify(row));
    }
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
  });
}
