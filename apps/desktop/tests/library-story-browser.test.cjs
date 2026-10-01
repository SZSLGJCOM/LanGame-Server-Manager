const assert = require("node:assert/strict");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

test("Steam story renders localized recovery, caches successful HTML and rejects obsolete responses in a real browser", { timeout: 90000 }, async () => {
  let mp4;
  const mediaRequests = [];
  const fixtureMiddleware = (request, response, next) => {
    if (request.url === "/__story_animation.mp4" && request.method === "POST") {
      const chunks = [];
      let bytes = 0;
      request.on("data", (chunk) => {
        bytes += chunk.length;
        if (bytes > 1024 * 1024) { request.destroy(); return; }
        chunks.push(chunk);
      });
      request.on("end", () => { mp4 = Buffer.concat(chunks); response.writeHead(204).end(); });
      return;
    }
    if (!["/__story_animation.webm", "/__story_animation.mp4"].includes(request.url)) return next();
    mediaRequests.push(request.url);
    if (request.url.endsWith(".webm")) { response.writeHead(404).end(); return; }
    if (!mp4) { response.writeHead(503).end(); return; }
    response.writeHead(200, { "Content-Type": "video/mp4", "Content-Length": mp4.length });
    response.end(mp4);
  };
  const report = await runBrowserFixture({ fixturePath: "library-story-browser.html", keyboard: true, fixtureMiddleware,
    viewport: { width: 1280, height: 900 }, screenshotPath: process.env.LANGAME_STORY_SCREENSHOT });
  assert.equal(report.status, "passed");
  assert.deepEqual(report.scenarios, ["video-source-types", "video-format-recovery", "loading", "zh-retry-keyboard", "html-sanitization", "success-cache-expiry",
    "saved-story-recovery", "null-saved-story-recovery", "sanitized-empty-saved-story-recovery", "en-retry", "null-retry", "obsolete-game", "obsolete-locale", "obsolete-null"]);
  assert.equal(mp4.subarray(4, 8).toString(), "ftyp", "Browser-generated alternate must be actual MP4 content");
  assert.equal(mediaRequests[0], "/__story_animation.webm");
  assert.ok(mediaRequests.includes("/__story_animation.mp4"), "Failed WebM must issue the real alternate MP4 request");
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`LIBRARY_STORY_BROWSER ${JSON.stringify(report)}`);
});
