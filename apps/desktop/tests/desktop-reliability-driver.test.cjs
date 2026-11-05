const test = require("node:test");
const assert = require("node:assert/strict");
const { recordTree, readHostEvidence } = require("../scripts/verify_desktop_reliability.cjs");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");

const processRow = (ProcessId, ParentProcessId, Created) => ({ ProcessId, ParentProcessId, Created });

test("native cleanup still observes children first sampled after their owner exits", () => {
  const observed = new Map();
  recordTree([processRow(42, 1, "100"), processRow(43, 42, "110")], 42, observed);
  recordTree([processRow(44, 43, "120"), processRow(45, 44, "130")], 42, observed);
  assert.deepEqual([...observed.keys()], [42, 43, 44, 45]);
});

test("reused host and browser PIDs cannot adopt an unrelated process tree", () => {
  const observed = new Map([[42, "100"], [43, "110"]]);
  recordTree([processRow(42, 1, "200"), processRow(43, 42, "210"), processRow(44, 43, "220")], 42, observed);
  assert.deepEqual([...observed], [[42, "100"], [43, "110"]]);
});

test("an observed parent cannot claim a process born before it", () => {
  const observed = new Map([[42, "100"]]);
  recordTree([processRow(50, 42, "90")], 42, observed);
  assert.equal(observed.has(50), false);
});

test("failed native host reads its actual cause without requiring a success report", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "langame-reliability-desktop-readback-"));
  try {
    const cause = { error: "Fixture page failed: Real console mount and native subscriptions exceeded 15000 ms",
      stage: "initial_command", command_count: 0, busy: false, output_markers: [],
      recovery: { paused: true, last_error: "WebView page load exceeded 15 seconds" },
      browser_process_ids: [], server_pid: 2 };
    const recoveryLog = `${JSON.stringify({ action: "desktop.webview.attempt_completed", context: {
      action: "Recreate", accepted: true, error: cause.recovery.last_error,
    } })}\n`;
    await fs.mkdir(path.join(root, "logs"));
    await fs.writeFile(path.join(root, "logs", "webview-recovery.jsonl"), recoveryLog, { flag: "wx" });
    await fs.writeFile(path.join(root, "failure.json"), JSON.stringify(cause), { flag: "wx" });
    const result = await readHostEvidence(root, { code: 1, signal: null });
    assert.deepEqual(result, { report: null, failure: cause, readError: null,
      recoveryLog, recoveryLogTruncated: false, recoveryLogReadError: null });
    const missing = await readHostEvidence(root, { code: null, signal: "SIGTERM" });
    assert.deepEqual(missing.failure, cause);
    await fs.unlink(path.join(root, "failure.json"));
    const killed = await readHostEvidence(root, { code: null, signal: "SIGTERM" });
    assert.equal(killed.report, null);
    assert.equal(killed.failure, null);
    assert.match(killed.readError, /failure\.json/);
    assert.equal(killed.recoveryLog, recoveryLog);
    await fs.writeFile(path.join(root, "logs", "webview-recovery.jsonl"), `${"x".repeat(128 * 1024)}${recoveryLog}`);
    const bounded = await readHostEvidence(root, { code: null, signal: "SIGTERM" });
    assert.equal(bounded.recoveryLogTruncated, true);
    assert.equal(Buffer.byteLength(bounded.recoveryLog), 128 * 1024);
    assert.ok(bounded.recoveryLog.endsWith(recoveryLog));
    await fs.unlink(path.join(root, "logs", "webview-recovery.jsonl"));
    const unavailable = await readHostEvidence(root, { code: null, signal: "SIGTERM" });
    assert.equal(unavailable.recoveryLog, null);
    assert.match(unavailable.recoveryLogReadError, /webview-recovery\.jsonl/);
    assert.match(unavailable.readError, /failure\.json/);
  } finally {
    assert.equal(path.dirname(root), path.resolve(os.tmpdir()));
    await fs.rm(root, { recursive: true, force: false });
  }
});
