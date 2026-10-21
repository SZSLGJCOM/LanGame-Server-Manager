const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { createHash } = require("node:crypto");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");

const snapshotPath = process.env.LANGAME_DST_MAP_SNAPSHOT_PATH;
const maximumBytes = 16 * 1024 * 1024;

async function readSnapshot(filename) {
  assert.ok(path.isAbsolute(filename), "The native snapshot path must be absolute");
  const file = await fs.open(filename, "r");
  try {
    const stat = await file.stat();
    assert.ok(stat.isFile() && stat.size > 0 && stat.size <= maximumBytes, "Native map snapshot must be a nonempty file no larger than 16 MiB");
    // Bound the actual read too, even if a concurrent writer grows the file.
    const buffer = Buffer.alloc(maximumBytes + 1);
    let size = 0;
    while (size < buffer.length) {
      const { bytesRead } = await file.read(buffer, size, buffer.length - size, size);
      if (!bytesRead) break;
      size += bytesRead;
    }
    assert.ok(size > 0 && size <= maximumBytes, "Native map snapshot exceeded the 16 MiB read limit");
    const bytes = buffer.subarray(0, size);
    const snapshot = JSON.parse(bytes.toString("utf8"));
    assert.ok(snapshot && typeof snapshot === "object" && !Array.isArray(snapshot), "Native snapshot must be a map response object");
    assert.ok(typeof snapshot.instanceId === "string" && snapshot.instanceId.length > 0, "Native snapshot must include its instance identity");
    assert.ok(["master", "caves", "islands", "volcano"].includes(snapshot.shard), "Native snapshot must identify a supported shard");
    return { bytes, sha256: createHash("sha256").update(bytes).digest("hex") };
  } finally { await file.close(); }
}

test("render a supplied native DST world and installed artwork without generated fixtures", {
  timeout: 120000,
  skip: !snapshotPath ? "LANGAME_DST_MAP_SNAPSHOT_PATH is required; native capture is opt-in" : false,
}, async () => {
  const snapshot = await readSnapshot(snapshotPath);
  const viewport = {
    width: Number(process.env.LANGAME_DST_MAP_VIEWPORT_WIDTH ?? 1280),
    height: Number(process.env.LANGAME_DST_MAP_VIEWPORT_HEIGHT ?? 900),
  };
  const focusPrefab = process.env.LANGAME_DST_MAP_FOCUS_PREFAB ?? null;
  assert.ok(focusPrefab === null || /^[A-Za-z0-9_]{1,128}$/.test(focusPrefab), "Native map focus must be a prefab identifier of 1–128 ASCII letters, digits or underscores");
  const zoom = Number(process.env.LANGAME_DST_MAP_ZOOM ?? (focusPrefab === null ? 1 : 4));
  assert.ok([.5, 1, 2, 4, 8, 10].includes(zoom), "Native screenshot zoom must use a production zoom preset");
  const options = Buffer.from(JSON.stringify({ focusPrefab, zoom }), "utf8");
  const screenshotRoot = process.env.LANGAME_TOOLS_SCREENSHOT_DIR ?? path.dirname(snapshotPath);
  assert.ok(path.isAbsolute(screenshotRoot), "Native map screenshot directory must be absolute");
  const screenshotPath = path.join(screenshotRoot, focusPrefab === null ? "dst-native-world.png" : "dst-native-world-focus.png");
  let reads = 0, optionReads = 0;
  const report = await runBrowserFixture({ fixturePath: "native-map-render-browser.html",
    viewport, screenshotPath, bundleWithMiddleware: true,
    fixtureMiddleware(request, response, next) {
      if (request.url === "/native-dst-map-render-options.json") {
        if (request.method !== "GET") { response.writeHead(405).end(); return; }
        optionReads++;
        response.writeHead(200, { "Content-Type": "application/json; charset=utf-8",
          "Cache-Control": "no-store", "Content-Length": options.length });
        response.end(options);
        return;
      }
      if (request.url !== "/native-dst-map-snapshot.json") return next();
      if (request.method !== "GET") { response.writeHead(405).end(); return; }
      reads++;
      response.writeHead(200, { "Content-Type": "application/json; charset=utf-8",
        "Cache-Control": "no-store", "Content-Length": snapshot.bytes.length });
      response.end(snapshot.bytes);
    },
  });
  assert.equal(report.status, "passed", report.error);
  assert.equal(reads, 1);
  assert.equal(optionReads, 1);
  if (focusPrefab === null) assert.equal(report.focus, null);
  else {
    assert.equal(report.focus?.prefab, focusPrefab);
    assert.equal(report.focus?.zoom, zoom);
    assert.equal(report.focus?.visible, true);
  }
  assert.ok(report.checks >= 10);
  assert.ok(report.landmarks >= 1 && report.texture_images >= 1 && report.loaded_images >= 2);
  assert.equal(report.offline, true);
  assert.equal(report.zoom, zoom);
  assert.ok(report.maximum_icon_pixels <= 24.1);
  assert.deepEqual(report.browser_errors, []);
  assert.equal(report.browser_exited, true);
  assert.equal(report.browser_processes_remaining, 0);
  assert.equal(report.scratch_removed, true);
  console.log(`DST_NATIVE_MAP_RENDER ${JSON.stringify({ ...report, snapshot_bytes: snapshot.bytes.length, snapshot_sha256: snapshot.sha256, screenshot: screenshotPath })}`);
});
