const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createRuntimeConsoleHistoryCache } = require("../src/runtime-console-history.ts");
const scope = (extra = {}) => ({ instanceId: "one", moduleId: "arksurvivalascended", processKey: "main", runId: 1, source: "game", ...extra });
const document = (lines = ["owned old line"]) => ({ source_path: "native/shared.log", lines, total_lines: lines.length, truncated: false, read_error: null });

test("revisited console history is isolated by instance, module, map, run and source even on the same native path", () => {
  const cache = createRuntimeConsoleHistoryCache();
  cache.write(scope(), document());
  for (const other of [{ instanceId: "two" }, { moduleId: "arksurvivalevolved" }, { processKey: "map-one" }, { runId: 2 }, { source: "console" }]) {
    assert.equal(cache.read(scope(other)), null);
  }
  assert.deepEqual(cache.read(scope()).lines, ["owned old line"]);
  cache.write(scope({ processKey: "map-one" }), document(["independent map"]));
  assert.deepEqual(cache.read(scope()).lines, ["owned old line"]);
  assert.deepEqual(cache.read(scope({ processKey: "map-one" })).lines, ["independent map"]);
});

test("history stays bounded, copies retained rows and evicts the least recently used documents", () => {
  const cache = createRuntimeConsoleHistoryCache();
  const incoming = document(Array.from({ length: 420 }, (_, index) => `line ${index}`));
  cache.write(scope(), incoming);
  incoming.lines.push("foreign mutation");
  const readback = cache.read(scope());
  assert.equal(readback.lines.length, 400);
  assert.equal(readback.lines[0], "line 20");
  readback.lines.push("readback mutation");
  assert.equal(cache.read(scope()).lines.length, 400);
  for (let runId = 2; runId <= 64; runId++) cache.write(scope({ runId }), document());
  cache.read(scope());
  cache.write(scope({ runId: 65 }), document());
  assert.equal(cache.read(scope({ runId: 2 })), null);
  assert.notEqual(cache.read(scope()), null);
  cache.write(scope({ runId: 66 }), document(["x".repeat(3 * 1024 * 1024)]));
  assert.equal(cache.read(scope({ runId: 66 })), null);
});

test("unknown runs are not reused and service recovery discards only its instance's cached documents", () => {
  const cache = createRuntimeConsoleHistoryCache();
  cache.write(scope({ runId: null }), document());
  assert.equal(cache.read(scope({ runId: null })), null);
  cache.write(scope(), document());
  cache.write(scope({ instanceId: "two" }), document());
  cache.clearInstance("one");
  assert.equal(cache.read(scope()), null);
  assert.notEqual(cache.read(scope({ instanceId: "two" })), null);
});

test("a restored document cannot reject current reads after an unobserved service restart", () => {
  const cache = createRuntimeConsoleHistoryCache();
  cache.write(scope(), { ...document(), snapshot_revision: 90 });
  const restored = cache.read(scope());
  assert.deepEqual(restored.lines, ["owned old line"]);
  assert.equal(restored.snapshot_revision, undefined);
});
