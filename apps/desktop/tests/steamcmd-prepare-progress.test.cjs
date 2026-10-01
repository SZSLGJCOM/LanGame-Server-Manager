const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
const { startSteamCmdPreparation, queuedSteamCmdProgress } = require("../src/steamcmd-prepare-operation.ts");
const { SteamCmdActivity, steamCmdDownloadPercent } = require("../src/components/SteamCmdActivity.tsx");
const { ZH_CN_UI_MESSAGES } = require("../src/i18n-messages-zh-ui.ts");
const { EN_US_EXTRA_MESSAGES } = require("../src/i18n-messages-en-extra.ts");

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));

function setup() {
  const ensure = deferred();
  const reads = [];
  const tasks = new Map();
  const values = [], statuses = [], errors = [], readErrors = [];
  let nextHandle = 0, settled = 0;
  const operation = startSteamCmdPreparation({
    operationId: "current", ensure: () => ensure.promise,
    read: (id) => { assert.equal(id, "current"); const result = deferred(); reads.push(result); return result.promise; },
    schedule: (callback, delay) => { assert.equal(delay, 1000); const handle = ++nextHandle; tasks.set(handle, callback); return handle; },
    cancel: (handle) => tasks.delete(handle), onProgress: (value) => values.push(value),
    onStatus: (value) => statuses.push(value), onError: (value) => errors.push(value),
    onReadError: (value) => readErrors.push(value), onSettled: () => { settled += 1; }
  });
  return { operation, ensure, reads, tasks, values, statuses, errors, readErrors, settled: () => settled,
    tick: () => { const [handle, callback] = tasks.entries().next().value; tasks.delete(handle); callback(); } };
}

test("preparation publishes queued immediately, polls without overlap and rejects another operation", async () => {
  const run = setup();
  assert.equal(run.values[0].phase, "queued");
  assert.equal(run.reads.length, 1);
  assert.equal(run.tasks.size, 0);
  run.reads[0].resolve({ ...queuedSteamCmdProgress("old"), phase: "downloading" });
  await flush();
  assert.equal(run.values.length, 1);
  run.tick();
  assert.equal(run.reads.length, 2);
  assert.equal(run.tasks.size, 0);
  run.reads[1].resolve({ ...queuedSteamCmdProgress("current"), phase: "downloading", downloaded_bytes: 400, total_bytes: 1000 });
  await flush();
  assert.equal(run.values[1].downloaded_bytes, 400);
  run.ensure.resolve({ executable_exists: true });
  await run.operation.finished;
  assert.equal(run.values[2].phase, "ready");
  assert.equal(run.values[2].active, false);
  assert.equal(run.statuses.length, 1);
  assert.equal(run.settled(), 1);
  assert.equal(run.tasks.size, 0);
});

test("completion invalidates a late progress response", async () => {
  const run = setup();
  run.ensure.resolve({ executable_exists: true });
  await run.operation.finished;
  run.reads[0].resolve({ ...queuedSteamCmdProgress("current"), phase: "updating" });
  await flush();
  assert.deepEqual(run.values.map((item) => item.phase), ["queued", "ready"]);
  assert.equal(run.tasks.size, 0);
});

test("backend-confirmed cancellation is stopped, never failed or ready", async () => {
  const run = setup();
  run.reads[0].resolve({ ...queuedSteamCmdProgress("current"), cancellable: true, cancel_requested: true });
  await flush();
  assert.equal(run.settled(), 0, "a cancellation acknowledgement must not complete the request");
  run.ensure.reject("installation_cancelled");
  await run.operation.finished;
  const stopped = run.values[run.values.length - 1];
  assert.equal(stopped.active, false);
  assert.equal(stopped.cancelled, true);
  assert.equal(stopped.error, null);
  assert.equal(run.errors.length, 0);
  assert.equal(run.statuses.length, 0);
  assert.equal(run.settled(), 1);
  assert.equal(run.tasks.size, 0);
});

test("unmount disposes readers and ignores late installation and polling callbacks", async () => {
  const run = setup();
  run.operation.dispose();
  run.reads[0].resolve(queuedSteamCmdProgress("current"));
  run.ensure.resolve({ executable_exists: true });
  await run.operation.finished;
  await flush();
  assert.equal(run.values.length, 1);
  assert.equal(run.statuses.length, 0);
  assert.equal(run.settled(), 0);
  assert.equal(run.tasks.size, 0);
});

test("transient progress failure is visible and retries; installation failure terminates reading", async () => {
  const run = setup();
  run.reads[0].reject(new Error("unavailable"));
  await flush();
  assert.equal(run.readErrors[0].message, "unavailable");
  assert.equal(run.tasks.size, 1);
  run.tick();
  const failure = JSON.stringify({ code: "steamcmd_preparation_stalled", message: "stalled", timeout_seconds: 90, output_excerpt: "Connecting" });
  run.ensure.reject(failure);
  await run.operation.finished;
  assert.equal(run.values[1].active, false);
  assert.equal(run.values[1].error, failure);
  assert.deepEqual(run.errors, [failure]);
  assert.equal(run.settled(), 1);
  run.reads[1].resolve(queuedSteamCmdProgress("current"));
  await flush();
  assert.equal(run.tasks.size, 0);
});

function render(snapshot, catalog = ZH_CN_UI_MESSAGES, message = "", extra = {}) {
  const t = (key, params = {}) => Object.entries(params).reduce((text, [name, value]) => text.replaceAll(`{${name}}`, String(value)), catalog[key] ?? key);
  return renderToStaticMarkup(React.createElement(SteamCmdActivity, { snapshot, message, locale: "zh-CN", t, ...extra }));
}

test("progress only reports a percentage for real download bytes with a known total", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), phase: "downloading", downloaded_bytes: 512, total_bytes: 1024, elapsed_seconds: 8 };
  assert.equal(steamCmdDownloadPercent(snapshot), 50);
  assert.match(render(snapshot), /role="progressbar"[^>]*aria-valuenow="50"/);
  assert.match(render(snapshot), /已用时 8 秒/);
  assert.match(render({ ...snapshot, phase: "updating" }), /aria-valuenow="50"/);
  for (const unknown of [{ total_bytes: null }, { total_bytes: 0 }, { downloaded_bytes: null }, { phase: "verifying" }]) {
    const value = { ...snapshot, ...unknown };
    assert.equal(steamCmdDownloadPercent(value), null);
    assert.match(render(value), /role="progressbar"/);
    assert.doesNotMatch(render(value), /aria-valuenow=/);
  }
});

test("SteamCMD cancellation uses the shared compact action and a distinct stopped result", () => {
  const current = { ...queuedSteamCmdProgress("one"), cancellable: true };
  assert.match(render(current, ZH_CN_UI_MESSAGES, "", { onStop() {} }), /class="shell-task-stop"[^>]*aria-label="停止"[^>]*><svg/);
  const stopping = render({ ...current, cancel_requested: true }, ZH_CN_UI_MESSAGES, "", { onStop() {} });
  assert.match(stopping, /disabled=""[^>]*aria-label="停止中…"/);
  assert.match(stopping, /role="progressbar"/);
  const stopped = render({ ...current, active: false, cancelled: true, cancellable: false });
  assert.match(stopped, /SteamCMD 准备已停止/);
  assert.doesNotMatch(stopped, /role="alert"|role="progressbar"|<button/);
});

test("activity shows waiting and error summaries with full diagnostics in its title", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), phase: "updating", idle_seconds: 20, detail: "Connecting to Steam", output_excerpt: "Old line\r\nConnecting to Steam\n" };
  const waiting = render(snapshot);
  assert.match(waiting, /正在等待 SteamCMD 输出/);
  assert.match(waiting, /当前验证.*独立验证.*15 分钟/);
  assert.match(waiting, /最近输出：.*Connecting to Steam/);
  assert.doesNotMatch(waiting, /Old line/);
  assert.match(waiting.replace(/<[^>]*>/g, ""), /等待响应/);
  assert.doesNotMatch(waiting.replace(/<[^>]*>/g, ""), /90 秒|15 分钟|独立验证|Connecting/);
  const failed = { ...snapshot, active: false, error: JSON.stringify({ code: "steamcmd_preparation_stalled", message: "stalled", timeout_seconds: 90, output_excerpt: "Connecting to Steam" }) };
  assert.match(render(failed), /role="alert"/);
  assert.match(render(failed), /连续 90 秒未产生进展/);
  assert.doesNotMatch(render(failed), /role="progressbar"|steamcmd_preparation_stalled/);
  assert.match(render(failed, EN_US_EXTRA_MESSAGES), /Check direct connectivity to Steam/);
  for (const phase of ["queued", "inspecting", "downloading", "extracting", "updating", "verifying", "ready"]) {
    assert.ok(ZH_CN_UI_MESSAGES[`steamcmd.prepare.${phase}`]);
    assert.ok(EN_US_EXTRA_MESSAGES[`steamcmd.prepare.${phase}`]);
  }
});

test("latest output uses the reported record instead of a pending console fragment", () => {
  const detail = "[2026-09-20 11:27:58] 正在下载更新 (已下载 397，共 10,673 KB)...";
  const snapshot = { ...queuedSteamCmdProgress("current"), phase: "updating", detail, output_excerpt: `${detail}\n[ 0%] 正在下载更新 (已下�` };
  assert.match(render(snapshot), /已下载 397，共 10,673 KB/);
  assert.doesNotMatch(render(snapshot), /�|\[ 0%\]/);
  assert.doesNotMatch(render({ ...snapshot, detail: "[ 0%] 已下�" }), /�|最近输出：/);
});

test("a progress read warning is visible in the activity text instead of only its title", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), phase: "updating", detail: "Connecting", output_excerpt: "Connecting" };
  const html = render(snapshot, ZH_CN_UI_MESSAGES, "进度暂时不可用，正在重试");
  const text = html.replace(/<[^>]*>/g, "");
  assert.match(text, /进度读取重试中/);
  assert.match(html, /title="[^"]*进度暂时不可用，正在重试/);
  assert.doesNotMatch(text, /Connecting/);
});

test("the two download stages identify the bootstrapper and self-update components", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), downloaded_bytes: 1024, total_bytes: 4096 };
  assert.match(render({ ...snapshot, phase: "downloading" }), /下载 SteamCMD 引导程序/);
  const updating = render({ ...snapshot, phase: "updating" });
  assert.match(updating, /更新 SteamCMD 组件/);
  assert.match(updating, /class="shell-task-speed"[^>]*aria-label="下载速度: —"/);
  assert.doesNotMatch(render({ ...snapshot, phase: "verifying" }), /aria-label="下载速度:/);
});

test("activity keeps console details in its title and shows plain elapsed text accessibly", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), phase: "verifying", detail: "Last download output", output_excerpt: "Last download output" };
  for (const seconds of [9, 10, 59, 60, 100, 3600]) {
    const html = render({ ...snapshot, elapsed_seconds: seconds });
    assert.match(html, new RegExp(`role="timer"[^>]*aria-label="已用时 ${seconds} 秒"[^>]*aria-live="off"`));
    assert.match(html.replace(/<[^>]*>/g, ""), new RegExp(`已用时 ${seconds} 秒`));
    assert.match(html, /最近输出： Last download output/);
    assert.doesNotMatch(html.replace(/<[^>]*>/g, ""), /Last download output/);
  }
});

test("waiting copy distinguishes installation lock, download and verification deadlines", () => {
  const snapshot = { ...queuedSteamCmdProgress("current"), idle_seconds: 20 };
  const queued = render(snapshot);
  assert.match(queued, /其他安装任务释放安装锁/);
  assert.doesNotMatch(queued, /90 秒|等待 SteamCMD 输出/);
  for (const phase of ["inspecting", "downloading", "extracting"]) {
    const html = render({ ...snapshot, phase });
    assert.match(html, /15 分钟/);
    assert.doesNotMatch(html, /90 秒|等待 SteamCMD 输出/);
  }
  const english = render({ ...snapshot, phase: "verifying" }, EN_US_EXTRA_MESSAGES);
  assert.match(english, /current verification stops/);
  assert.match(english, /one separate verification may run automatically/);
  assert.match(render(snapshot, EN_US_EXTRA_MESSAGES), /release the installation lock/);
});
