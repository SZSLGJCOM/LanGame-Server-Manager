const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { createHash } = require("node:crypto");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

function options(capture = null, pageCapture) {
  return {
    fixturePath: "curseforge-reference-browser.html",
    viewport: { width: 1560, height: 1000 },
    deviceScaleFactor: capture ? 1 : 1.5,
    fixtureCleanup: true,
    bundleWithMiddleware: true,
    fixtureMiddleware(request, response, next) {
      if (request.url === "/__curseforge_page_capture.png" && pageCapture) {
        response.writeHead(200, { "Content-Type": "image/png", "Cache-Control": "no-store" }).end(pageCapture.bytes);
        return;
      }
      if (request.url !== "/__curseforge_reference_fixture") return next();
      response.writeHead(200, { "Content-Type": "application/json" })
        .end(JSON.stringify({ capture, locale: capture ? "zh-CN" : "en-US", pageCapture: pageCapture?.evidence }));
    },
  };
}

test("ASA recognition stays hidden until verification and releases cancelled or completed native hosts", { timeout: 90000 }, async () => {
  const report = await runBrowserFixture(options());
  assert.equal(report.status, "passed", report.error);
  assert.equal(report.saves, 1);
  assert.equal(report.resolutions, 5);
  assert.deepEqual(report.sessions.map(({ scenario, closed, resolved }) => ({ scenario, closed, resolved })), [
    { scenario: "store-background-cancel", closed: 1, resolved: true },
    { scenario: "store-verification-cancel", closed: 1, resolved: true },
    { scenario: "my-mods-success", closed: 1, resolved: true },
    { scenario: "navigation-cancel", closed: 1, resolved: true },
    { scenario: "unmount-cancel", closed: 1, resolved: true },
    { scenario: "late-open", closed: 1, resolved: false },
  ]);
  assert.equal(report.physical_bounds_checked, true);
  assert.equal(report.resize_checked, true);
  assert.equal(report.modal_occlusion_checked, true);
  assert.equal(report.hidden_by_default, true);
  assert.equal(report.batch_loading_hides_verification, true);
  assert.equal(report.completed_page_hidden_before_result, true);
  assert.ok(report.success_auto_dismiss_ms >= 4800 && report.success_auto_dismiss_ms < 6500);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
});

const output = process.env.LANGAME_CURSEFORGE_SCREENSHOT_DIR;
const pageCapturePath = process.env.LANGAME_CURSEFORGE_PAGE_CAPTURE;
if (output) for (const capture of ["background", "success", ...(pageCapturePath ? ["verification"] : [])]) {
  test(`Chinese CurseForge ${capture} state uses real workbench and activity components`, { timeout: 90000 }, async () => {
    assert.equal(path.isAbsolute(output), true, "Screenshot directory must be absolute");
    await fs.mkdir(output, { recursive: true });
    let pageCapture;
    if (capture === "verification") {
      assert.equal(path.isAbsolute(pageCapturePath), true, "Native page capture must be an absolute external path");
      const repository = path.resolve(__dirname, "../../..");
      const relative = path.relative(repository, pageCapturePath);
      assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Native page evidence must stay outside the repository");
      const stat = await fs.stat(pageCapturePath);
      assert.ok(stat.isFile() && stat.size >= 24 && stat.size <= 20 * 1024 * 1024, "Native evidence must be a bounded PNG file");
      const bytes = await fs.readFile(pageCapturePath);
      assert.ok(bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
        && bytes.subarray(12, 16).toString("ascii") === "IHDR", "Native evidence must contain a PNG header");
      pageCapture = { bytes, evidence: { filename: path.basename(pageCapturePath),
        sha256: createHash("sha256").update(bytes).digest("hex"), width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) } };
    }
    const name = `curseforge-quiet-${capture}-zh-CN-1560x1000`;
    const report = await runBrowserFixture({ ...options(capture, pageCapture), screenshotPath: path.join(output, name + ".png") });
    assert.equal(report.status, "passed", report.error);
    assert.equal(report.locale, "zh-CN");
    assert.equal(report.screenshot_state, capture);
    assert.equal(report.native_boundary, capture === "verification" ? "native-page-capture-preview" : "mocked");
    if (capture === "verification") {
      assert.equal(report.verification_trigger, "simulated");
      assert.deepEqual(report.page_capture, pageCapture.evidence);
      assert.equal(report.disclosure, "测试场景，网页为真实加载截图，未触发真实验证码");
    }
    assert.equal(report.saves, capture === "success" ? 1 : 0);
    assert.equal(report.sessions.length, 1);
    assert.deepEqual(report.browser_errors, []);
    assert.equal(report.browser_exited, true);
    assert.equal(report.browser_processes_remaining, 0);
    assert.equal(report.scratch_removed, true);
    await fs.writeFile(path.join(output, name + ".json"), JSON.stringify(report, null, 2));
  });
}
