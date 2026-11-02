const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { appendRuntimeLogStreamEvent } = require("../src/runtime-log-stream.ts");

const options = { instanceId: "fixture", maxLines: 400 };
const event = (revision, lines = [], partial = [], extra = {}) => ({ instance_id: "fixture", log_path: "fixture/main.log",
  process_key: "main", run_id: 1, lines, byte_offset: revision, emitted_at_unix_ms: 100 + revision,
  console: { stream_id: "main-1", started_at_unix_ms: 100, revision, previous_revision: revision - 1,
    reset: revision === 1, partial_lines: partial }, ...extra });

test("partial output is replaced until committed, with identity-based replay protection", () => {
  let current = appendRuntimeLogStreamEvent(null, event(1, [], ["Down"]), options);
  assert.deepEqual(current.console_sources[0].partial_lines, ["Down"]);
  current = appendRuntimeLogStreamEvent(current, event(2, [], ["Downloading 12%"]), options);
  assert.deepEqual(current.lines, []);
  assert.deepEqual(current.console_sources[0].partial_lines, ["Downloading 12%"]);
  current = appendRuntimeLogStreamEvent(current, event(3, ["Downloading 12%", "repeat", "repeat"]), options);
  assert.deepEqual(current.lines, ["Downloading 12%", "repeat", "repeat"]);
  assert.deepEqual(current.console_sources[0].partial_lines, []);
  assert.equal(appendRuntimeLogStreamEvent(current, event(3, ["Downloading 12%", "repeat", "repeat"]), options), current);
});

test("interleaved shard commits preserve arrival order while each partial is independently replaced", () => {
  const aggregate = { ...options, followLatestPath: true, mergeProcessLogs: true };
  const cave = (revision, lines, partial) => event(revision, lines, partial, { log_path: "fixture/caves.log", process_key: "caves",
    console: { stream_id: "caves-1", started_at_unix_ms: 101, revision, previous_revision: revision - 1,
      reset: revision === 1, partial_lines: partial } });
  let current = appendRuntimeLogStreamEvent(null, event(1, ["main first"], ["main pending"]), aggregate);
  current = appendRuntimeLogStreamEvent(current, cave(1, ["caves first"], ["caves pending"]), aggregate);
  current = appendRuntimeLogStreamEvent(current, event(2, ["main pending"], []), aggregate);
  assert.deepEqual(current.lines, ["[main] main first", "[caves] caves first", "[main] main pending"]);
  assert.deepEqual(current.console_sources.map(source => source.partial_lines), [[], ["caves pending"]]);
});

test("a missing revision is not guessed into place and an authoritative snapshot resumes the stream", () => {
  const { mergeRuntimeConsoleSnapshot, runtimeConsoleEventNeedsRefresh } = require("../src/runtime-console-stream.ts");
  let current = appendRuntimeLogStreamEvent(null, event(1, ["first"]), options);
  const gap = event(3, ["third"], ["pending"]);
  assert.equal(runtimeConsoleEventNeedsRefresh(current, gap), true);
  assert.equal(appendRuntimeLogStreamEvent(current, gap, options), current);
  const source = { ...current.console_sources[0], revision: 3, lines: ["first", "second", "third"], partial_lines: ["pending"], total_lines: 3 };
  current = mergeRuntimeConsoleSnapshot(current, { source_path: "fixture/main.log", lines: [], console_sources: [source] }, options);
  current = appendRuntimeLogStreamEvent(current, event(4, ["pending"]), options);
  assert.deepEqual(current.lines, ["first", "second", "third", "pending"]);
  assert.equal(runtimeConsoleEventNeedsRefresh(current, gap), false);
});

test("late snapshots and events cannot roll back a newer revision or file generation", () => {
  const { mergeRuntimeConsoleSnapshot } = require("../src/runtime-console-stream.ts");
  let current = appendRuntimeLogStreamEvent(null, event(1, ["old first"]), options);
  const old = { ...current, console_sources: current.console_sources.map(source => ({ ...source })) };
  current = appendRuntimeLogStreamEvent(current, event(2, ["old second"]), options);
  assert.equal(mergeRuntimeConsoleSnapshot(current, old, options), current);
  const rotated = event(1, ["new generation"], [], { console: { stream_id: "main-2", started_at_unix_ms: 200,
    revision: 1, previous_revision: 0, reset: true, partial_lines: [] } });
  current = appendRuntimeLogStreamEvent(current, rotated, options);
  assert.deepEqual(current.lines, ["new generation"]);
  assert.equal(mergeRuntimeConsoleSnapshot(current, old, options), current);
  assert.equal(appendRuntimeLogStreamEvent(current, event(9, ["late old generation"]), options), current);
});

test("bounded retention counts committed lines without treating partial updates as new rows", () => {
  const bounded = { ...options, maxLines: 3 };
  let current = appendRuntimeLogStreamEvent(null, event(1, ["a", "b", "c", "d"], ["partial"]), bounded);
  current = appendRuntimeLogStreamEvent(current, event(2, [], ["partial updated"]), bounded);
  assert.deepEqual(current.lines, ["b", "c", "d"]);
  assert.equal(current.total_lines, 4);
  assert.equal(current.truncated, true);
});

test("startup recovery excludes prior source generations and previously owned paths", () => {
  const { mergeRuntimeConsoleSnapshot, runtimeConsoleDisplayLines } = require("../src/runtime-console-stream.ts");
  const base = { log_path: "fixture/current.log", process_key: "main", display_name: "Main", run_id: 1,
    stream_id: "new", started_at_unix_ms: 101, revision: 2, lines: ["new completed"], partial_lines: ["new partial"], total_lines: 1, truncated: false };
  const recovered = mergeRuntimeConsoleSnapshot(null, { lines: [], console_sources: [
    { ...base, log_path: "fixture/previous.log", stream_id: "old-known", started_at_unix_ms: 102, lines: ["known previous path"] },
    { ...base, log_path: "fixture/unknown-old.log", stream_id: "old-unknown", started_at_unix_ms: 99, lines: ["previous generation"] }, base
  ] }, { ...options, mergeProcessLogs: true, startupBoundary: { startedAtUnixMs: 100, previousLogPaths: new Set(["fixture/previous.log"]) } });
  assert.deepEqual(runtimeConsoleDisplayLines(recovered, true), ["[main] new completed", "[main] new partial"]);
});

test("large completed-line bursts bound both source documents and ordered rows by content bytes", () => {
  const { RUNTIME_CONSOLE_TEXT_BYTES } = require("../src/runtime-console-stream.ts");
  let current = null;
  for (let revision = 1; revision <= 8; revision++) current = appendRuntimeLogStreamEvent(current,
    event(revision, [`${revision}:` + "x".repeat(150000)]), options);
  const bytes = rows => rows.reduce((total, line) => total + line.length * 2, 0);
  assert.ok(bytes(current.console_sources[0].lines) <= RUNTIME_CONSOLE_TEXT_BYTES);
  assert.ok(bytes(current.console_rows.map(row => row.line)) <= RUNTIME_CONSOLE_TEXT_BYTES);
  assert.ok(current.lines.at(-1).startsWith("8:"));
  assert.equal(current.total_lines, 8);
  assert.equal(current.truncated, true);
});

test("failed source readback errors remain visible until that source has a successful authoritative snapshot", () => {
  const { mergeRuntimeConsoleSnapshot, markRuntimeConsoleSourceForRefresh } = require("../src/runtime-console-stream.ts");
  let current = appendRuntimeLogStreamEvent(null, event(1, [], ["unfinished"]), options);
  current = markRuntimeConsoleSourceForRefresh(current, "fixture/main.log");
  let source = { ...current.console_sources[0], revision: 2, read_error: "FILE_READ_FAILED" };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.equal(current.read_error, "FILE_READ_FAILED");
  assert.deepEqual(current.console_refresh_paths, ["fixture/main.log"]);
  source = { ...source, revision: 3, lines: ["completed on disk"], partial_lines: [], total_lines: 1, read_error: null };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, ["completed on disk"]);
  assert.deepEqual(current.console_refresh_paths, []);
  assert.equal(current.read_error, null);
});

test("a full document restores history at the same revision without accepting older readbacks", () => {
  const { mergeRuntimeConsoleSnapshot } = require("../src/runtime-console-stream.ts");
  const lines = Array.from({ length: 400 }, (_, index) => `row ${index}`);
  const full = appendRuntimeLogStreamEvent(null, event(1, lines, ["pending"]), options);
  const source = full.console_sources[0];
  const tail = { ...source, revision: 2, lines: lines.slice(-32), truncated: true };
  let current = mergeRuntimeConsoleSnapshot(null, { lines: [], console_sources: [tail] }, options);
  assert.equal(current.lines.length, 32);
  assert.equal(mergeRuntimeConsoleSnapshot(current, full, options), current);
  current = mergeRuntimeConsoleSnapshot(current, { ...full, console_sources: [{ ...source, revision: 2 }] }, options);
  assert.deepEqual(current.lines, lines);
  assert.deepEqual(current.console_sources[0].partial_lines, ["pending"]);
  assert.equal(current.total_lines, 400);
  const restored = current;
  assert.equal(mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [tail] }, options), restored);
});

test("newer overlapping tails preserve the retained prefix and repeated lines by position", () => {
  const { mergeRuntimeConsoleSnapshot } = require("../src/runtime-console-stream.ts");
  const lines = Array.from({ length: 410 }, (_, index) => `row ${Math.floor(index / 2)}`);
  let current = appendRuntimeLogStreamEvent(null, event(1, lines.slice(0, 400), ["before"]), options);
  const source = { ...current.console_sources[0], revision: 2, lines: lines.slice(-32), total_lines: 410,
    partial_lines: ["after"], truncated: true };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, lines.slice(-400));
  assert.deepEqual(current.console_sources[0].lines, lines.slice(-400));
  assert.deepEqual(current.console_sources[0].partial_lines, ["after"]);
  assert.equal(current.total_lines, 410);
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [{ ...source, revision: 3,
    partial_lines: ["after updated"] }] }, options);
  assert.deepEqual(current.lines, lines.slice(-400));
  assert.deepEqual(current.console_sources[0].partial_lines, ["after updated"]);
});

test("adjacent source windows join but missing line ranges and new generations do not invent history", () => {
  const { mergeRuntimeConsoleSnapshot } = require("../src/runtime-console-stream.ts");
  let current = appendRuntimeLogStreamEvent(null, event(1, ["same", "same"]), options);
  let source = { ...current.console_sources[0], revision: 2, lines: ["same", "third"], total_lines: 4, truncated: true };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, ["same", "same", "same", "third"]);
  source = { ...source, revision: 3, lines: ["after gap"], total_lines: 6 };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, ["after gap"]);
  assert.equal(current.total_lines, 6);
  source = { ...source, stream_id: "main-2", started_at_unix_ms: 200, revision: 1, lines: ["new generation"], total_lines: 1 };
  current = mergeRuntimeConsoleSnapshot(current, { lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, ["new generation"]);
});

test("filtering an unchanged source materializes only its rows and removes process prefixes", () => {
  const { mergeRuntimeConsoleSnapshot } = require("../src/runtime-console-stream.ts");
  const aggregate = { ...options, followLatestPath: true, mergeProcessLogs: true };
  let current = appendRuntimeLogStreamEvent(null, event(1, ["main first"]), aggregate);
  current = appendRuntimeLogStreamEvent(current, event(1, ["other"], [], { log_path: "fixture/caves.log", process_key: "caves",
    console: { stream_id: "caves-1", started_at_unix_ms: 101, revision: 1, previous_revision: 0, reset: true, partial_lines: [] } }), aggregate);
  current = appendRuntimeLogStreamEvent(current, event(2, ["main second"]), aggregate);
  const source = current.console_sources.find(source => source.stream_id === "main-1");
  current = mergeRuntimeConsoleSnapshot(current, { source_path: "fixture/main.log", lines: [], console_sources: [source] }, options);
  assert.deepEqual(current.lines, ["main first", "main second"]);
  assert.deepEqual(current.console_sources, [source]);
  assert.deepEqual(current.console_rows.map(row => row.stream_id), ["main-1", "main-1"]);
  assert.equal(current.total_lines, 2);
});

test("source selection normalizes paths, retains repeated source rows, and removes startup prefixes", () => {
  const { selectRuntimeConsoleSource, runtimeConsoleDisplayLines } = require("../src/runtime-console-stream.ts");
  const aggregate = { ...options, followLatestPath: true, mergeProcessLogs: true };
  let current = appendRuntimeLogStreamEvent(null, event(1, ["first", "repeat"], ["pending"]), aggregate);
  current = appendRuntimeLogStreamEvent(current, event(1, ["other"], ["other pending"], { log_path: "fixture/caves.log", process_key: "caves",
    console: { stream_id: "caves-1", started_at_unix_ms: 101, revision: 1, previous_revision: 0, reset: true, partial_lines: ["other pending"] } }), aggregate);
  current = appendRuntimeLogStreamEvent(current, event(2, ["repeat"], ["pending"]), aggregate);
  const selected = selectRuntimeConsoleSource(current, "FIXTURE\\\\main.log/", 2);
  assert.deepEqual(selected.lines, ["repeat", "repeat"]);
  assert.deepEqual(selected.console_sources.map(source => source.stream_id), ["main-1"]);
  assert.deepEqual(selected.console_rows, [{ stream_id: "main-1", line: "repeat" }, { stream_id: "main-1", line: "repeat" }]);
  assert.equal(selected.source_path, "fixture/main.log");
  assert.equal(selected.total_lines, 3);
  assert.deepEqual(runtimeConsoleDisplayLines(selected, false), ["repeat", "repeat", "pending"]);
  assert.equal(selectRuntimeConsoleSource(current, "fixture/missing.log"), null);
});

test("source selection preserves a matching plain snapshot and rejects other or absent sources", () => {
  const { selectRuntimeConsoleSource } = require("../src/runtime-console-stream.ts");
  const current = { source_path: "fixture/main.log", lines: ["plain line"] };
  assert.equal(selectRuntimeConsoleSource(current, "FIXTURE\\main.log"), current);
  assert.equal(selectRuntimeConsoleSource(current, "fixture/other.log"), null);
  assert.equal(selectRuntimeConsoleSource(null, "fixture/main.log"), null);
  assert.equal(selectRuntimeConsoleSource({ lines: ["unknown source"] }, "fixture/main.log"), null);
});
