const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { appendRuntimeStartupEvent, mergeRuntimeStartupSession, runtimeStartupBeforeCursor, runtimeStartupLines,
  withoutRuntimeStartupSource, runtimeStartupEventNeedsRefresh, restoreRuntimeStartupConsole } = require("../src/runtime-console-startup-session.ts");

const boundary = { instanceId: "a", startupAttemptId: "attempt-a", startedAtUnixMs: 100, previousLogPaths: new Set(["old.log"]) };
const source = (revision, lines, total_lines = lines.length, partial_lines = []) => ({
  log_path: "startup/a.log", process_key: "startup", display_name: "Startup", run_id: null,
  stream_id: "startup-a", started_at_unix_ms: 101, revision, lines, total_lines, partial_lines, truncated: total_lines > lines.length
});
const receipt = (lines, extra = {}) => ({ attempt_id: "attempt-a", status: "starting", started_at_unix_ms: 100,
  finished_at_unix_ms: null, source_path: "startup/a.log", run_ids: [], error: null, reader_error: null,
  snapshot: { source_path: "startup/a.log", lines, total_lines: lines.length }, console_source: null,
  before_cursor: null, retained_start_cursor: "0", line_generation: "stream:startup-a", ...extra });
const event = (revision, lines = [], partial_lines = [], extra = {}) => ({ instance_id: "a", process_key: "startup", run_id: null,
  log_path: "startup/a.log", lines, byte_offset: revision * 100, emitted_at_unix_ms: 101 + revision,
  console: { stream_id: "startup-a", started_at_unix_ms: 101, revision, previous_revision: revision - 1,
    reset: revision === 1, partial_lines }, ...extra });

test("active startup progress replaces in place; identical complete rows and final outcome survive handoff", () => {
  let state = appendRuntimeStartupEvent(null, event(1, ["self check", "repeat", "repeat"], ["validate 10%"]), boundary);
  state = appendRuntimeStartupEvent(state, event(2, [], ["validate 99%"]), boundary);
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["self check", "repeat", "repeat", "validate 99%"]);
  state = appendRuntimeStartupEvent(state, event(3, ["validate complete"]), boundary);
  const final = receipt(["self check", "repeat", "repeat", "validate complete", "[200] [startup] succeeded"], {
    status: "succeeded", run_ids: [7, 8], finished_at_unix_ms: 200
  });
  state = mergeRuntimeStartupSession(state, final);
  assert.deepEqual(runtimeStartupLines(state, "gap"), final.snapshot.lines);
  assert.equal(state.session.status, "succeeded");
  assert.equal(appendRuntimeStartupEvent(state, event(2, [], ["stale progress"]), boundary), state);
  assert.deepEqual(runtimeStartupLines(mergeRuntimeStartupSession(null, final), "gap"), final.snapshot.lines, "Reload reads the same durable result");
});

test("failed, cancelled and interrupted attempts remain readable without any run and reject previous receipts", () => {
  for (const status of ["failed", "cancelled", "interrupted"]) {
    const final = receipt(["validation", `[200] [startup] ${status}`], { status, finished_at_unix_ms: 200,
      error: status === "failed" ? "validation failed" : null, reader_error: "[150] [reader] retained error" });
    let state = mergeRuntimeStartupSession(null, final, { boundary });
    assert.equal(state.session.status, status);
    assert.deepEqual(state.session.run_ids, []);
    state = mergeRuntimeStartupSession(state, receipt(["old attempt"], { attempt_id: "old", started_at_unix_ms: 50 }), { boundary });
    assert.deepEqual(runtimeStartupLines(state, "gap"), final.snapshot.lines);
    assert.equal(state.session.reader_error, "[150] [reader] retained error");
    assert.equal(mergeRuntimeStartupSession(null, final, { boundary: { ...boundary, startupAttemptId: "attempt-b" } }), null);
  }
});

test("a delayed initial read cannot roll back newer events, replay rows, or revive finished progress", () => {
  let state = appendRuntimeStartupEvent(null, event(1, ["same", "same"], ["10%"]), boundary);
  state = appendRuntimeStartupEvent(state, event(2, ["done"], ["20%"]), boundary);
  state = mergeRuntimeStartupSession(state, receipt(["same", "same"], { console_source: source(1, ["same", "same"], 2, ["10%"]) }), { boundary });
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["same", "same", "done", "20%"]);
  state = mergeRuntimeStartupSession(state, receipt(["same", "same", "done", "end"], { status: "succeeded", finished_at_unix_ms: 200 }));
  state = mergeRuntimeStartupSession(state, receipt(["same", "same"], { console_source: source(1, ["same", "same"], 2, ["10%"]) }));
  assert.equal(state.session.status, "succeeded");
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["same", "same", "done", "end"]);
  assert.equal(runtimeStartupEventNeedsRefresh(state, event(4, ["missing predecessor"])), true);
});

test("late mounts restore the capture's startup cursor before a delayed initial receipt", () => {
  const captured = { lines: ["[startup] complete"], console_sources: [source(3, ["complete"], 1, ["90%"]) ] };
  let state = restoreRuntimeStartupConsole(null, captured, boundary);
  state = mergeRuntimeStartupSession(state, receipt(["complete"], { console_source: source(1, ["complete"], 1, ["10%"]) }), { boundary });
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["complete", "90%"]);
  assert.equal(state.live.console_sources[0].revision, 3);
});

test("paging and current tails merge by positions, retain expanded history, and fill middle gaps", () => {
  const rows = Array.from({ length: 1000 }, (_, index) => index === 399 || index === 400 ? "repeated boundary" : `line ${index}`);
  const page = (start, end, before_cursor, extra = {}) => receipt(rows.slice(start, end), {
    snapshot: { lines: rows.slice(start, end), total_lines: end }, before_cursor, ...extra
  });
  let state = mergeRuntimeStartupSession(null, page(600, 1000, "6000"));
  state = mergeRuntimeStartupSession(state, page(200, 600, "2000"), { page: true });
  state = mergeRuntimeStartupSession(state, page(0, 200, null), { page: true });
  assert.deepEqual(runtimeStartupLines(state, "gap"), rows);
  assert.equal(runtimeStartupBeforeCursor(state), null);
  state = mergeRuntimeStartupSession(state, page(600, 1000, "6000"));
  assert.deepEqual(runtimeStartupLines(state, "gap"), rows, "Routine refresh does not collapse expanded pages to 400 rows");
  state = mergeRuntimeStartupSession(state, receipt(["after gap"], { snapshot: { lines: ["after gap"], total_lines: 1002 }, before_cursor: "10010" }));
  assert.equal(runtimeStartupBeforeCursor(state), "10010");
  assert.deepEqual(runtimeStartupLines(state, "gap").slice(-2), ["gap", "after gap"]);
  state = mergeRuntimeStartupSession(state, receipt(["missing row"], { snapshot: { lines: ["missing row"], total_lines: 1001 }, before_cursor: "10000" }), { page: true });
  assert.deepEqual(runtimeStartupLines(state, "gap"), [...rows, "missing row", "after gap"]);
  assert.equal(runtimeStartupBeforeCursor(state), null);
});

test("log retention generation changes discard shifted page positions and foreign page responses", () => {
  let state = mergeRuntimeStartupSession(null, receipt(["old", "old"]));
  state = mergeRuntimeStartupSession(state, receipt(["retained new"], { retained_start_cursor: "100" }));
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["retained new"]);
  const foreign = receipt(["foreign page"], { attempt_id: "other", started_at_unix_ms: 300 });
  assert.equal(mergeRuntimeStartupSession(state, foreign, { page: true }), state);
});

test("reader generations reset page coordinates while an initial disk read cannot displace newer streamed output", () => {
  let state = appendRuntimeStartupEvent(null, event(1, ["first", "second"], ["90%"]), boundary);
  const disk = receipt(["first"], { line_generation: "retained:0" });
  assert.equal(mergeRuntimeStartupSession(state, disk, { readGeneration: null, readRetainedStartCursor: null }), state,
    "The request began before this stream existed and cannot revoke its cursor");
  state = mergeRuntimeStartupSession(state, disk, { readGeneration: "stream:startup-a", readRetainedStartCursor: "0" });
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["first"], "A known service/cache reset establishes new disk coordinates");
  state = appendRuntimeStartupEvent(state, event(1, ["new reader"], [], { console: {
    stream_id: "startup-b", started_at_unix_ms: 300, revision: 1, previous_revision: 0, reset: true, partial_lines: []
  } }), boundary);
  assert.deepEqual(runtimeStartupLines(state, "gap"), ["new reader"]);
  assert.equal(state.session.line_generation, "stream:startup-b");
});

test("late pages and tails cannot restore an obsolete retained range or reader generation", () => {
  const old = receipt(["old tail"], { snapshot: { lines: ["old tail"], total_lines: 800 }, before_cursor: "7000" });
  let state = mergeRuntimeStartupSession(null, old);
  const readBasis = { readGeneration: state.session.line_generation, readRetainedStartCursor: state.session.retained_start_cursor };
  state = mergeRuntimeStartupSession(state, receipt(["current tail"], {
    snapshot: { lines: ["current tail"], total_lines: 400 }, retained_start_cursor: "4000", line_generation: "retained:4000"
  }));
  const oldPage = { ...old, snapshot: { lines: ["expired page"], total_lines: 400 }, before_cursor: null };
  assert.equal(mergeRuntimeStartupSession(state, oldPage, { page: true, ...readBasis }), state,
    "A page requested before the reader restart cannot replace the new tail");

  state = mergeRuntimeStartupSession(null, old);
  state = mergeRuntimeStartupSession(state, receipt(["current tail"], {
    snapshot: { lines: ["current tail"], total_lines: 801 }, retained_start_cursor: "4000"
  }));
  assert.equal(mergeRuntimeStartupSession(state, old, readBasis), state,
    "Retention can advance without changing stream ID; a delayed tail must not restore its old prefix");
  assert.equal(mergeRuntimeStartupSession(state, oldPage, { page: true, ...readBasis }), state);
});

test("startup-only source is removed from aggregate exactly once without hiding selected game rows", () => {
  const startup = source(1, ["startup same", "startup same"]);
  const game = { ...source(2, ["game same", "game same"]), log_path: "game/a.log", process_key: "main", stream_id: "game-a" };
  const snapshot = { source_path: "startup/a.log", lines: [], console_sources: [startup, game],
    console_rows: [{ stream_id: startup.stream_id, line: "startup same" }, { stream_id: game.stream_id, line: "game same" },
      { stream_id: startup.stream_id, line: "startup same" }, { stream_id: game.stream_id, line: "game same" }] };
  const filtered = withoutRuntimeStartupSource(snapshot, "STARTUP\\a.log", true);
  assert.deepEqual(filtered.lines, ["[main] game same", "[main] game same"]);
  assert.deepEqual(filtered.console_sources, [game]);
  assert.equal(withoutRuntimeStartupSource({ source_path: "startup/a.log", lines: ["startup same"] }, "startup/a.log"), null);
});
