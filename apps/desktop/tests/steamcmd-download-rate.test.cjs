const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { sampleDownloadRate, downloadBytesPerSecond } = require("../src/download-rate.ts");

function progress(seconds, bytes, changes = {}) {
  return { operation_id: "one", active: true, phase: "downloading", downloaded_bytes: bytes,
    total_bytes: 1000000, elapsed_seconds: seconds, idle_seconds: 0, detail: "", output_excerpt: "", error: null, ...changes };
}

test("archive and self-update speeds use recent byte increments, never initial accumulated bytes", () => {
  for (const phase of ["downloading", "updating"]) {
    let snapshot = progress(10, 100000, { phase });
    let window = sampleDownloadRate(null, snapshot);
    assert.equal(downloadBytesPerSecond(window, snapshot), null);
    for (let seconds = 11; seconds <= 20; seconds++) {
      snapshot = progress(seconds, 100000 + (seconds - 10) * 2048, { phase });
      window = sampleDownloadRate(window, snapshot);
      assert.equal(downloadBytesPerSecond(window, snapshot), 2048);
      assert.ok(window.samples.length <= 6);
    }
  }
});

test("fresh stalled progress settles at zero, while missing fresh snapshots hide a stale speed", () => {
  let window = sampleDownloadRate(null, progress(0, 0));
  let snapshot = progress(1, 5000);
  window = sampleDownloadRate(window, snapshot);
  assert.equal(downloadBytesPerSecond(window, snapshot), 5000);
  assert.equal(downloadBytesPerSecond(window, snapshot, 3), null);
  for (let seconds = 2; seconds <= 6; seconds++) {
    snapshot = progress(seconds, 5000);
    window = sampleDownloadRate(window, snapshot);
  }
  assert.equal(downloadBytesPerSecond(window, snapshot), 0);
  snapshot = progress(30, 10000);
  window = sampleDownloadRate(window, snapshot);
  assert.equal(downloadBytesPerSecond(window, snapshot), null);
});

test("new operations, update packages, phase changes and counter resets cannot inherit a speed", () => {
  const start = sampleDownloadRate(null, progress(4, 1000));
  const previous = sampleDownloadRate(start, progress(5, 2000));
  for (const changes of [
    { operation_id: "two" }, { phase: "updating" }, { total_bytes: 2000000 },
    { downloaded_bytes: 100 }, { elapsed_seconds: 0 }
  ]) {
    const snapshot = progress(6, 3000, changes);
    const window = sampleDownloadRate(previous, snapshot);
    assert.equal(window.samples.length, 1);
    assert.equal(downloadBytesPerSecond(window, snapshot), null);
  }
  for (const changes of [{ phase: "extracting" }, { phase: "verifying" }, { active: false },
    { downloaded_bytes: null }, { downloaded_bytes: -1 }, { downloaded_bytes: Infinity }]) {
    const snapshot = progress(6, 3000, changes);
    assert.equal(sampleDownloadRate(previous, snapshot), null);
    assert.equal(downloadBytesPerSecond(previous, snapshot), null);
  }
});

test("same-second polls cannot divide by zero and unknown totals still support measured speed", () => {
  let snapshot = progress(0, 1000, { total_bytes: null });
  let window = sampleDownloadRate(null, snapshot);
  snapshot = progress(0, 1500, { total_bytes: null });
  window = sampleDownloadRate(window, snapshot);
  assert.equal(downloadBytesPerSecond(window, snapshot), null);
  snapshot = progress(2, 2500, { total_bytes: null });
  window = sampleDownloadRate(window, snapshot);
  assert.equal(downloadBytesPerSecond(window, snapshot), 500);
});
