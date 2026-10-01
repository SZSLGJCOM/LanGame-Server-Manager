const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

for (const scenario of [
  { locale: "zh-CN", width: 960, height: 600 },
  { locale: "en-US", width: 960, height: 600 },
  { locale: "zh-CN", width: 1560, height: 900 },
  { locale: "en-US", width: 1560, height: 900 },
  { locale: "zh-CN", width: 960, height: 600, captureHelp: true },
]) {
  test(`Astroneer creation layout and controls at ${scenario.width}x${scenario.height} ${scenario.locale}${scenario.captureHelp ? " with help" : ""}`,
    { timeout: 90000 }, async () => {
      const outputRoot = process.env.LANGAME_LIBRARY_CREATION_SCREENSHOT_DIR;
      if (outputRoot) await fs.mkdir(outputRoot, { recursive: true });
      const report = await runBrowserFixture({
        fixturePath: "library-creation-layout-browser.html", keyboard: true,
        viewport: { width: scenario.width, height: scenario.height },
        screenshotPath: outputRoot ? path.join(outputRoot,
          `library-creation-${scenario.width}x${scenario.height}-${scenario.locale}${scenario.captureHelp ? "-help" : ""}.png`) : undefined,
        contentSecurityPolicy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'",
        fixtureMiddleware(request, response, next) {
          if (request.url === "/__library_creation_art.svg") {
            response.writeHead(200, { "Content-Type": "image/svg+xml" });
            response.end('<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900"><rect width="1600" height="900" fill="#172c43"/><circle cx="1130" cy="265" r="105" fill="#d4eff1"/><path d="M0 580L350 400L610 575L940 340L1600 660V900H0" fill="#315c73"/><path d="M0 760L540 565L810 745L1160 540L1600 710V900H0" fill="#4c8c99"/><text x="90" y="175" fill="#edf7f6" font-family="sans-serif" font-size="68">ASTRONEER</text><text x="95" y="235" fill="#b1dce2" font-family="sans-serif" font-size="26">LOCAL LAYOUT FIXTURE</text></svg>');
            return;
          }
          if (request.url !== "/__library_creation_config") return next();
          response.writeHead(200, { "Content-Type": "application/json" });
          response.end(JSON.stringify({ locale: scenario.locale, captureHelp: scenario.captureHelp ?? false }));
        },
      });
      assert.equal(report.status, "passed");
      assert.ok(report.checks >= 70);
      assert.deepEqual(report.browser_errors, []);
      assert.equal(report.browser_processes_remaining, 0);
      assert.equal(report.scratch_removed, true);
      console.log(`LIBRARY_CREATION_LAYOUT ${JSON.stringify(report)}`);
    });
}
