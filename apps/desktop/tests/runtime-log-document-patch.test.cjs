const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { appendRuntimeLogStreamEvent, retainNewestRuntimeGameDocument } = require("../src/runtime-log-stream.ts");
const options = { instanceId: "a", maxLines: 400 };
const patch = (revision, start, delete_count, lines, extra = {}) => ({ instance_id: "a", log_path: "native/game.log",
  lines: [], byte_offset: revision, emitted_at_unix_ms: revision,
  document_patch: { previous_revision: revision - 1, revision, start, delete_count, lines,
    total_lines: start + lines.length, truncated: false, read_error: null, ...extra } });

test("native document patches replace an unfinished last row and preserve real duplicate completed rows", () => {
  let current = { source_path: "native/game.log", snapshot_revision: 5, lines: ["first", "load"] };
  current = appendRuntimeLogStreamEvent(current, patch(6, 1, 1, ["loading 10%"]), options);
  assert.deepEqual(current.lines, ["first", "loading 10%"]);
  const append = patch(7, 2, 0, ["repeat", "repeat"]);
  current = appendRuntimeLogStreamEvent(current, append, options);
  assert.deepEqual(current.lines, ["first", "loading 10%", "repeat", "repeat"]);
  assert.equal(appendRuntimeLogStreamEvent(current, append, options), current);
  current = appendRuntimeLogStreamEvent(current, patch(8, 0, 4, ["repeat", "repeat", "tail"], { total_lines: 5, truncated: true }), options);
  assert.deepEqual(current.lines, ["repeat", "repeat", "tail"]);
  assert.equal(current.total_lines, 5);
  assert.equal(current.truncated, true);
});

test("native document gaps and mismatched retained ranges require recovery before any splice", () => {
  const { runtimeLogDocumentPatchNeedsRefresh } = require("../src/runtime-log-document-patch.ts");
  let current = { source_path: "native/game.log", snapshot_revision: 5, lines: ["known"] };
  const missing = patch(7, 2, 0, ["seven"]);
  assert.equal(runtimeLogDocumentPatchNeedsRefresh(current, missing), true);
  assert.equal(appendRuntimeLogStreamEvent(current, missing, options), current);
  assert.equal(runtimeLogDocumentPatchNeedsRefresh(current, patch(6, 32, 1, ["bad range"])), true);
  current = retainNewestRuntimeGameDocument(current, { source_path: "native/game.log", snapshot_revision: 7,
    lines: ["known", "six", "seven"] }, 400);
  current = appendRuntimeLogStreamEvent(current, patch(8, 3, 0, ["eight"]), options);
  assert.deepEqual(current.lines, ["known", "six", "seven", "eight"]);
  assert.equal(retainNewestRuntimeGameDocument(current, { source_path: "native/game.log", snapshot_revision: 7, lines: ["late read"] }, 400), current);
});
