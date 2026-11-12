const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = () => {};
const { observeArkToolsInstallation, queuedArkToolsInstallation, parseArkToolsInstallProgress } = require("../src/ark-tools-installation.ts");
const { ArkToolsInstallActivity } = require("../src/components/ArkToolsInstallActivity.tsx");
const queued = () => queuedArkToolsInstallation("ark-a", "install-a");
const ready = { installed: true, updateAvailable: false, connected: false, canUninstall: true, issue: null };
const receipt = (elapsedSeconds = 8) => ({ ...ready, progress: { ...queued(), phase: "ready", active: false, elapsedSeconds } });
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const flush = () => new Promise(resolve => setImmediate(resolve));
function setup(recovery = false, readStatus = async () => ready) {
  const prepared = deferred(), reads = [], values = [], statuses = [], warnings = [], timers = new Map();
  let nextTimer = 0, settled = 0, preparations = 0;
  const observer = observeArkToolsInstallation({
    initial: queued(), prepare: recovery ? undefined : () => { preparations++; return prepared.promise; },
    read: () => { const read = deferred(); reads.push(read); return read.promise; }, readStatus,
    schedule: (callback, delay) => { assert.equal(delay, 1000); const id = ++nextTimer; timers.set(id, callback); return id; },
    cancel: id => timers.delete(id), onProgress: value => values.push(value), onStatus: value => statuses.push(value),
    onIssue: value => warnings.push(value), onSettled: () => { settled++; }
  });
  return { observer, prepared, reads, values, statuses, warnings, timers, settled: () => settled, preparations: () => preparations,
    tick() { const [id, callback] = timers.entries().next().value; timers.delete(id); callback(); } };
}

test("install progress validates operation identity, byte values and terminal invariants", () => {
  assert.deepEqual(parseArkToolsInstallProgress(queued(), "ark-a", "install-a"), queued());
  assert.equal(parseArkToolsInstallProgress(null, "ark-a", null), null);
  for (const change of [{ instanceId: "ark-b" }, { operationId: "other" }, { active: false },
    { phase: "unknown" }, { totalBytes: -1 }, { totalBytes: 0 }, { downloadedBytes: Number.NaN },
    { phase: "failed", active: false, error: null }, { error: "unexpected" }]) {
    assert.throws(() => parseArkToolsInstallProgress({ ...queued(), ...change }, "ark-a", "install-a"), /Invalid ARK installation progress/);
  }
  assert.equal(parseArkToolsInstallProgress({ ...queued(), active: false, phase: "failed", error: "TLS connection failed" }, "ark-a", null).error,
    "TLS connection failed");
});

test("installation shows queued immediately, reads without overlap and ignores another operation", async () => {
  const run = setup();
  assert.equal(run.values[0].phase, "queued");
  assert.equal(run.reads.length, 1);
  assert.equal(run.timers.size, 0);
  run.reads[0].resolve({ ...queued(), operationId: "previous", phase: "downloading" });
  await flush();
  assert.equal(run.values.length, 1);
  run.tick();
  run.reads[1].resolve({ ...queued(), phase: "downloading", source: "api.gitcode.com", downloadedBytes: 512, totalBytes: 1024 });
  await flush();
  assert.equal(run.values.at(-1).downloadedBytes, 512);
  run.prepared.resolve(receipt());
  await run.observer.finished;
  assert.equal(run.values.at(-1).phase, "ready");
  assert.deepEqual(run.statuses, [ready]);
  assert.equal(run.settled(), 1);
  assert.equal(run.timers.size, 0);
});

test("native failure preserves diagnostics and the final elapsed time", async () => {
  const run = setup();
  run.prepared.reject("api.gitcode.com: connection reset; github.com: timed out");
  await flush();
  run.reads[0].resolve({ ...queued(), phase: "failed", active: false, elapsedSeconds: 21, error: "api.gitcode.com: connection reset; github.com: timed out" });
  await run.observer.finished;
  const failure = run.values.at(-1);
  assert.equal(failure.phase, "failed");
  assert.equal(failure.elapsedSeconds, 21);
  assert.match(failure.error, /connection reset.*timed out/);
  await flush();
  assert.equal(run.values.at(-1), failure);
  assert.equal(run.timers.size, 0);
  assert.equal(run.statuses.length, 0);
});

test("a confirmed failed snapshot cannot be replaced by a late success receipt", async () => {
  const run = setup();
  run.reads[0].resolve({ ...queued(), phase: "failed", active: false, error: "verification failed" });
  await run.observer.finished;
  run.prepared.resolve(receipt());
  await flush();
  assert.equal(run.values.at(-1).phase, "failed");
  assert.equal(run.values.at(-1).error, "verification failed");
  assert.equal(run.statuses.length, 0);
  assert.equal(run.settled(), 1);
});

test("transient read failure remains observable without completing or repeating installation", async () => {
  const run = setup();
  run.reads[0].reject(new Error("progress pipe unavailable"));
  await flush();
  assert.deepEqual(run.warnings.at(-1), { kind: "progress", message: "progress pipe unavailable" });
  assert.equal(run.settled(), 0);
  run.tick();
  run.reads[1].resolve({ ...queued(), phase: "verifying" });
  await flush();
  assert.equal(run.warnings.at(-1), null);
  run.prepared.resolve(receipt());
  await run.observer.finished;
});

test("disposing reads ignores late native results and never cancels the installer", async () => {
  const run = setup();
  run.observer.dispose();
  run.reads[0].resolve({ ...queued(), phase: "downloading" });
  run.prepared.resolve(receipt());
  await run.observer.finished;
  await flush();
  assert.equal(run.values.length, 1);
  assert.equal(run.statuses.length, 0);
  assert.equal(run.settled(), 0);
  assert.equal(run.timers.size, 0);
});

test("a recovered installation observes its existing terminal outcome", async () => {
  const run = setup(true);
  run.reads[0].resolve({ ...queued(), phase: "ready", active: false });
  await run.observer.finished;
  assert.deepEqual(run.statuses, [ready]);
  assert.equal(run.settled(), 1);
  const failed = setup(true);
  failed.reads[0].resolve({ ...queued(), phase: "failed", active: false, error: "checksum mismatch" });
  await failed.observer.finished;
  assert.equal(failed.values.at(-1).error, "checksum mismatch");
  assert.equal(failed.statuses.length, 0);
});

test("a success receipt supplies final elapsed time even before a pending progress read returns", async () => {
  const run = setup();
  assert.equal(run.values.at(-1).elapsedSeconds, 0);
  run.prepared.resolve(receipt(8));
  await run.observer.finished;
  assert.equal(run.values.at(-1).elapsedSeconds, 8);
  assert.equal(run.values.at(-1).active, false);
  const terminal = run.values.at(-1);
  run.reads[0].resolve({ ...queued(), phase: "downloading", elapsedSeconds: 1 });
  await flush();
  assert.equal(run.values.at(-1), terminal, "a late active read cannot change the completed timer");
  assert.equal(run.timers.size, 0);
});

test("lost prepare receipt keeps observing the same active operation and never resubmits", async () => {
  const run = setup();
  run.prepared.reject("Runtime connection ended after the request was sent (broken pipe); inspect task state before retrying");
  await flush();
  assert.equal(run.settled(), 0);
  assert.equal(run.warnings.at(-1).kind, "receipt");
  run.reads[0].resolve({ ...queued(), phase: "downloading", downloadedBytes: 512, totalBytes: 1024 });
  await flush();
  assert.equal(run.values.at(-1).active, true);
  assert.equal(run.values.at(-1).error, null);
  run.tick();
  run.reads[1].resolve({ ...queued(), phase: "ready", active: false });
  await run.observer.finished;
  assert.equal(run.values.at(-1).phase, "ready");
  assert.deepEqual(run.statuses, [ready]);
  assert.equal(run.preparations(), 1);
});

test("an unreadable or absent progress record does not turn an unknown receipt into confirmed failure", async () => {
  const run = setup();
  run.prepared.reject("Runtime response timed out; the operation may still be running");
  await flush();
  run.reads[0].reject(new Error("runtime restarting"));
  await flush();
  assert.equal(run.settled(), 0);
  assert.equal(run.values.at(-1).active, true);
  assert.match(run.warnings.at(-1).message, /runtime restarting/);
  run.tick();
  run.reads[1].resolve(null);
  await flush();
  assert.equal(run.settled(), 0);
  assert.equal(run.values.at(-1).phase, "queued");
  run.tick();
  run.reads[2].resolve({ ...queued(), phase: "failed", active: false, error: "download checksum mismatch" });
  await run.observer.finished;
  assert.equal(run.values.at(-1).error, "download checksum mismatch");
  assert.equal(run.preparations(), 1);
});

test("a confirmed preflight rejection completes after a fresh missing-operation read", async () => {
  const run = setup();
  run.prepared.reject("Invalid ARK extension installation identity.");
  await flush();
  run.reads[0].resolve(null);
  await flush();
  assert.equal(run.settled(), 0, "the read that started before the rejection cannot establish absence");
  run.tick();
  run.reads[1].resolve(null);
  await run.observer.finished;
  assert.equal(run.values.at(-1).phase, "failed");
  assert.equal(run.values.at(-1).error, "Invalid ARK extension installation identity.");
  assert.equal(run.timers.size, 0);
});

test("a transport-confirmed unsent operation does not wait forever when the service is unreachable", async () => {
  const run = setup();
  run.prepared.reject("Cannot reach the runtime service: unavailable. No operation was sent.");
  await flush();
  run.reads[0].reject(new Error("still unreachable"));
  await run.observer.finished;
  assert.equal(run.values.at(-1).phase, "failed");
  assert.match(run.values.at(-1).error, /No operation was sent/);
  assert.equal(run.preparations(), 1);
});

test("a ready native outcome retries status inspection without claiming installation failure", async () => {
  let statusReads = 0;
  const run = setup(true, async () => { if (++statusReads === 1) throw new Error("status pipe unavailable"); return ready; });
  run.reads[0].resolve({ ...queued(), phase: "ready", active: false, elapsedSeconds: 32 });
  await flush();
  assert.equal(run.settled(), 0);
  assert.equal(run.values.at(-1).phase, "ready");
  assert.equal(run.values.at(-1).active, false);
  assert.equal(run.values.at(-1).elapsedSeconds, 32, "status inspection must not restart a completed native clock");
  assert.equal(run.warnings.at(-1).kind, "verification");
  run.tick();
  run.reads[1].resolve({ ...queued(), phase: "ready", active: false, elapsedSeconds: 32 });
  await run.observer.finished;
  assert.equal(run.values.at(-1).phase, "ready");
  assert.equal(run.values.at(-1).elapsedSeconds, 32);
  assert.equal(statusReads, 2);
});

test("install activity uses real bytes and indeterminate progress for other stages", () => {
  const render = (snapshot, locale = "zh-CN", issue = null) => renderToStaticMarkup(React.createElement(ArkToolsInstallActivity, { snapshot, locale, issue }));
  const downloading = { ...queued(), phase: "downloading", source: "api.gitcode.com", downloadedBytes: 524288, totalBytes: 1048576 };
  assert.match(render(downloading), /role="progressbar"[^>]*aria-valuenow="50"/);
  assert.match(render(downloading), /512 KB \/ 1 MB/);
  assert.match(render(downloading), /api.gitcode.com/);
  for (const snapshot of [{ ...downloading, totalBytes: null }, { ...downloading, phase: "verifying" }, queued()]) {
    const html = render(snapshot);
    assert.match(html, /role="progressbar"/);
    assert.doesNotMatch(html, /aria-valuenow=/);
  }
  assert.match(render(downloading, "en-US", { kind: "progress", message: "pipe unavailable" }), /Retrying progress check/);
  assert.match(render(downloading, "zh-CN", { kind: "receipt", message: "pipe closed after request" }), /安装回执未确认，继续检查/);
  const failure = render({ ...downloading, phase: "failed", active: false, error: "TLS connection failed" });
  assert.match(failure, /服务端扩展安装失败/);
  assert.match(failure, /TLS connection failed/);
  assert.doesNotMatch(failure, /role="progressbar"/);
});
