const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function loadRuntimeLogStream() {
  const sourcePath = path.join(__dirname, "..", "src", "runtime-log-stream.ts");
  const source = fs.readFileSync(sourcePath, "utf8");
  const outputText = transpileTypeScript(source, sourcePath);

  const module = { exports: {} };
  vm.runInNewContext(outputText, {
    module,
    exports: module.exports,
    require
  }, { filename: sourcePath });
  return module.exports;
}

function appendEvents(events) {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  return events.reduce((snapshot, event) => appendRuntimeLogStreamEvent(snapshot, {
    instance_id: "demo", process_key: "main", run_id: 1, log_path: "fixture/current.log",
    byte_offset: 100, emitted_at_unix_ms: 1, ...event
  }, { instanceId: "demo", logPath: "fixture/current.log", maxLines: 400 }), null);
}

test("complete lines and the final unterminated tail can share one byte offset and timestamp", () => {
  const snapshot = appendEvents([{ lines: ["complete line"] }, { lines: ["unterminated tail"] }]);
  assert.deepEqual(Array.from(snapshot.lines), ["complete line", "unterminated tail"]);
  assert.equal(snapshot.total_lines, 2);
});

test("native document events atomically replace the bounded snapshot without replay or text deduplication", () => {
  const snapshot = appendEvents([{ lines: ["older snapshot"] }, { lines: [], snapshot: {
    source_path: "fixture/current.log", lines: ["game started", "game started"], total_lines: 2
  } }]);
  assert.deepEqual(Array.from(snapshot.lines), ["game started", "game started"]);
  assert.equal(snapshot.total_lines, 2);
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const foreign = appendRuntimeLogStreamEvent(snapshot, { instance_id: "demo", log_path: "fixture/other-map.log",
    lines: [], snapshot: { source_path: "fixture/other-map.log", lines: ["other"] }, byte_offset: 10, emitted_at_unix_ms: 1 },
  { instanceId: "demo", logPath: "fixture/current.log", maxLines: 400 });
  assert.equal(foreign, snapshot);
});

test("native document revisions reject late invoke responses and late events on the same source", () => {
  const { retainNewestRuntimeGameDocument, appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const current = { source_path: "fixture/current.log", snapshot_revision: 9, lines: ["newest"] };
  assert.equal(retainNewestRuntimeGameDocument(current,
    { source_path: "fixture/current.log", snapshot_revision: 8, lines: ["late response"] }, 400), current);
  const lateEvent = appendRuntimeLogStreamEvent(current, { instance_id: "demo", log_path: "fixture/current.log",
    lines: [], snapshot_revision: 7, snapshot: { source_path: "fixture/current.log", lines: ["late event"] },
    byte_offset: 0, emitted_at_unix_ms: 100 }, { instanceId: "demo", logPath: "fixture/current.log", maxLines: 400 });
  assert.equal(lateEvent, current);
  const next = retainNewestRuntimeGameDocument(current,
    { source_path: "fixture/current.log", snapshot_revision: 10, lines: ["next", "next"] }, 400);
  assert.deepEqual(Array.from(next.lines), ["next", "next"]);
  assert.equal(retainNewestRuntimeGameDocument(next,
    { source_path: "fixture/other.log", snapshot_revision: 1, lines: ["other source"] }, 400).source_path, "fixture/other.log");
});

test("identical complete and final lines remain distinct without a publisher event identity", () => {
  // Reading `a\na` can emit two ["a"] payloads at the same cursor in one millisecond.
  const snapshot = appendEvents([{ lines: ["a"] }, { lines: ["a"] }]);
  assert.deepEqual(Array.from(snapshot.lines), ["a", "a"]);
  assert.equal(snapshot.total_lines, 2);
});

test("native rows resembling diagnostics remain unchanged at a reused cursor", () => {
  const diagnostics = ["[LanGame] Console stream incomplete.", "[LanGame] Log read failed: access denied"];
  const snapshot = appendEvents([{ lines: ["ready"] }, ...diagnostics.map(line => ({ lines: [line] }))]);
  assert.deepEqual(Array.from(snapshot.lines), ["ready", ...diagnostics]);
});

test("an independent stream error does not become native file content", () => {
  const snapshot = appendEvents([{ lines: ["native output"] },
    { lines: [], stream_error: "Reader failed", byte_offset: 0 }]);
  assert.deepEqual(Array.from(snapshot.lines), ["native output"]);
});

test("same-path file rotation may reset the byte offset without a service reset", () => {
  const snapshot = appendEvents([
    { lines: ["before rotation"], byte_offset: 4000, emitted_at_unix_ms: 200 },
    { lines: ["rotated file first line"], byte_offset: 24, emitted_at_unix_ms: 201 },
    { lines: ["rotated file next line"], byte_offset: 48, emitted_at_unix_ms: 202 }
  ]);
  assert.deepEqual(Array.from(snapshot.lines), ["before rotation", "rotated file first line", "rotated file next line"]);
});

test("a lower cursor or timestamp alone does not establish that a matching event is stale", () => {
  const snapshot = appendEvents([
    { lines: ["current delivery"], byte_offset: 4000, emitted_at_unix_ms: 200 },
    { lines: ["delayed delivery"], byte_offset: 20, emitted_at_unix_ms: 100 }
  ]);
  assert.deepEqual(Array.from(snapshot.lines), ["current delivery", "delayed delivery"]);
});

test("a matching selected source replaces an existing snapshot from another source", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const prior = { source_path: "fixture/native.log", lines: ["native file output"], total_lines: 1, truncated: false };
  const event = { instance_id: "demo", process_key: "main", log_path: "fixture/managed.log",
    lines: ["selected managed output"], byte_offset: 20, emitted_at_unix_ms: 1 };
  const next = appendRuntimeLogStreamEvent(prior, event,
    { instanceId: "demo", logPath: event.log_path, maxLines: 400 });
  assert.equal(next.source_path, event.log_path);
  assert.deepEqual(Array.from(next.lines), ["selected managed output"]);
  assert.equal(next.total_lines, 1);

  const aggregate = appendRuntimeLogStreamEvent(prior, event,
    { instanceId: "demo", logPath: event.log_path, mergeProcessLogs: true, maxLines: 400 });
  assert.deepEqual(Array.from(aggregate.lines), ["native file output", "[main] selected managed output"]);
});

test("startup aggregate retains both shard streams with explicit labels", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  let snapshot = null;
  for (const [index, [process_key, text]] of [["master", "Generating forest"], ["caves", "Generating caves"], ["master", "Surface ready"]].entries()) {
    snapshot = appendRuntimeLogStreamEvent(snapshot, {
      instance_id: "demo", process_key, log_path: `fixture/${process_key}.log`,
      lines: [text], byte_offset: (index + 1) * 100, emitted_at_unix_ms: 1
    }, { instanceId: "demo", followLatestPath: true, mergeProcessLogs: true, maxLines: 2 });
  }
  assert.deepEqual(Array.from(snapshot.lines), ["[caves] Generating caves", "[master] Surface ready"]);
  assert.equal(snapshot.total_lines, 3);
  assert.equal(snapshot.truncated, true);
});

test("startup excludes prior shard paths even when their final output is emitted after the new click", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const startupBoundary = { instanceId: "demo", startedAtUnixMs: 100,
    previousLogPaths: new Set(["fixture/run-1-main.log", "fixture/run-1-caves.log"]) };
  const options = { instanceId: "demo", startupBoundary, followLatestPath: true,
    mergeProcessLogs: true, maxLines: 20 };
  let snapshot = null;
  for (const [log_path, emitted_at_unix_ms] of [
    ["FIXTURE\\run-1-main.log", 101], ["fixture/run-1-caves.log", 102], ["fixture/queued-unknown.log", 99]
  ]) {
    snapshot = appendRuntimeLogStreamEvent(snapshot, { instance_id: "demo", log_path,
      process_key: "main", lines: ["old session output"], byte_offset: 20, emitted_at_unix_ms }, options);
    assert.equal(snapshot, null);
  }
  for (const process_key of ["main", "caves", "extra-shard"]) {
    snapshot = appendRuntimeLogStreamEvent(snapshot, { instance_id: "demo", process_key,
      log_path: `fixture/run-2-${process_key}.log`, lines: ["new output"], byte_offset: 20,
      emitted_at_unix_ms: 100 }, options);
  }
  assert.deepEqual(Array.from(snapshot.lines), ["[main] new output", "[caves] new output", "[extra-shard] new output"]);
});

test("startup path collection includes primary, sibling, and retained runs with normalized paths", () => {
  const { collectRuntimeLogPaths } = loadRuntimeLogStream();
  const paths = collectRuntimeLogPaths({ active_run: { log_path: "D:\\logs\\main.log",
    processes: [{ log_path: "D:/logs/caves.log" }, { log_path: null }] } }, {
    log_tail: { source_path: "D:/LOGS/main.log" },
    recent_runs: [{ log_path: "D:/logs/prior.log", processes: [{ log_path: "D:/logs/prior-caves.log" }] }]
  });
  assert.deepEqual(Array.from(paths), ["d:/logs/main.log", "d:/logs/caves.log", "d:/logs/prior.log", "d:/logs/prior-caves.log"]);
});

test("appendRuntimeLogStreamEvent appends matching lines and caps the tail", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const snapshot = {
    source_path: "D:/LanGame/instances/demo/logs/run-1.log",
    total_lines: 2,
    lines: ["old-1", "old-2"],
    truncated: false,
    read_error: null
  };
  const event = {
    instance_id: "demo",
    process_key: "main",
    log_path: "D:\\LanGame\\instances\\demo\\logs\\run-1.log",
    lines: ["new-1", "new-2", "new-3"],
    byte_offset: 120,
    emitted_at_unix_ms: 1234
  };

  const next = appendRuntimeLogStreamEvent(snapshot, event, {
    instanceId: "demo",
    logPath: "D:/LanGame/instances/demo/logs/run-1.log",
    maxLines: 4
  });

  assert.deepEqual(Array.from(next.lines), ["old-2", "new-1", "new-2", "new-3"]);
  assert.equal(next.total_lines, 5);
  assert.equal(next.truncated, true);
  assert.equal(next.read_error, null);
});

test("appendRuntimeLogStreamEvent ignores events for another instance or log path", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const snapshot = {
    source_path: "D:/LanGame/instances/demo/logs/run-1.log",
    total_lines: 1,
    lines: ["old"],
    truncated: false,
    read_error: null
  };

  assert.equal(
    appendRuntimeLogStreamEvent(snapshot, {
      instance_id: "other",
      process_key: "main",
      log_path: "D:/LanGame/instances/demo/logs/run-1.log",
      lines: ["ignored"],
      byte_offset: 10,
      emitted_at_unix_ms: 1
    }, {
      instanceId: "demo",
      logPath: "D:/LanGame/instances/demo/logs/run-1.log",
      maxLines: 20
    }),
    snapshot
  );

  assert.equal(
    appendRuntimeLogStreamEvent(snapshot, {
      instance_id: "demo",
      process_key: "main",
      log_path: "D:/LanGame/instances/demo/logs/other.log",
      lines: ["ignored"],
      byte_offset: 10,
      emitted_at_unix_ms: 1
    }, {
      instanceId: "demo",
      logPath: "D:/LanGame/instances/demo/logs/run-1.log",
      maxLines: 20
    }),
    snapshot
  );
});

test("appendRuntimeLogStreamEvent attaches to the first startup log without a selected path", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const event = {
    instance_id: "demo",
    process_key: null,
    log_path: "D:/LanGame/instances/demo/logs/run-2.log",
    lines: ["Checking SteamCMD runtime..."],
    byte_offset: 42,
    emitted_at_unix_ms: 2
  };

  const next = appendRuntimeLogStreamEvent(null, event, {
    instanceId: "demo",
    logPath: null,
    followLatestPath: true,
    maxLines: 20
  });

  assert.equal(next.source_path, event.log_path);
  assert.deepEqual(Array.from(next.lines), event.lines);
});

test("appendRuntimeLogStreamEvent switches away from a stale run during startup", () => {
  const { appendRuntimeLogStreamEvent } = loadRuntimeLogStream();
  const snapshot = {
    source_path: "D:/LanGame/instances/demo/logs/run-1.log",
    total_lines: 1,
    lines: ["old run"],
    truncated: false,
    read_error: null
  };
  const event = {
    instance_id: "demo",
    process_key: null,
    log_path: "D:/LanGame/instances/demo/logs/run-2.log",
    lines: ["Preparing startup..."],
    byte_offset: 42,
    emitted_at_unix_ms: 2
  };

  const next = appendRuntimeLogStreamEvent(snapshot, event, {
    instanceId: "demo",
    followLatestPath: true,
    maxLines: 20
  });

  assert.equal(next.source_path, event.log_path);
  assert.deepEqual(Array.from(next.lines), event.lines);
  assert.equal(next.total_lines, 1);
});
