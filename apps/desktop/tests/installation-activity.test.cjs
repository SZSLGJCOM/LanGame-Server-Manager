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
const { selectActiveInstallationJob, installationJobPercent, completedInstallationJob } = require("../src/installation-job.ts");
const { InstallationActivity } = require("../src/components/InstallationActivity.tsx");
const { ZH_CN_UI_MESSAGES } = require("../src/i18n-messages-zh-ui.ts");
const { EN_US_EXTRA_MESSAGES } = require("../src/i18n-messages-en-extra.ts");
const job = (changes = {}) => ({ id: "install-game", label: "Install game", kind: "DownloadGame", status: "Running",
  cancellable: true, cancel_requested: false,
  target_id: "game", progress_percent: 12, detail: "Raw diagnostic", output_excerpt: "Console output",
  install_progress: { phase: "downloading", downloaded_bytes: 512 * 1024, total_bytes: 1024 * 1024, percent: 50, elapsed_seconds: 8 }, ...changes });
function render(value, catalog = ZH_CN_UI_MESSAGES, extra = {}) {
  const t = (key, params = {}) => Object.entries(params).reduce((text, [key, value]) => text.replaceAll(`{${key}}`, String(value)), catalog[key] ?? key);
  return renderToStaticMarkup(React.createElement(InstallationActivity, { job: value, name: "Game", locale: "zh-CN", t, ...extra }));
}

test("global footer chooses active installation over queued work and excludes unrelated jobs", () => {
  const queued = job({ id: "queued", install_progress: { phase: "queued" } });
  const running = job();
  const unrelated = job({ id: "other", kind: "StartInstance" });
  assert.equal(selectActiveInstallationJob([unrelated, queued, running]), running);
  assert.equal(selectActiveInstallationJob([unrelated, queued]), queued);
  assert.equal(selectActiveInstallationJob([unrelated, job({ status: "Completed" })]), null);
});

test("the footer offers stop only for controllable work and keeps progress while stopping", () => {
  const normal = render(job(), ZH_CN_UI_MESSAGES, { onStop() {} });
  assert.match(normal, /<button[^>]*class="shell-task-stop"[^>]*aria-label="停止"[^>]*><svg/);
  assert.ok(normal.indexOf('class="shell-task-stop"') > normal.indexOf('class="shell-task-elapsed"'));
  assert.doesNotMatch(normal.replace(/<[^>]*>/g, ""), /停止/);
  const stopping = render(job({ cancel_requested: true }), ZH_CN_UI_MESSAGES, { onStop() {} });
  assert.match(stopping, /<button[^>]*disabled=""[^>]*aria-label="停止中…"[^>]*aria-busy="true"/);
  assert.match(stopping, /role="progressbar"/);
  assert.doesNotMatch(render(job({ cancellable: false }), ZH_CN_UI_MESSAGES, { onStop() {} }), /<button/);
  assert.doesNotMatch(render(job({ status: "Cancelled" }), ZH_CN_UI_MESSAGES, { onStop() {} }), /<button|role="progressbar"/);
});

test("preparation shows the actual Steam connection or configuration step without invented progress", () => {
  for (const [detail, expected] of [
    ["Connecting anonymously to Steam Public...", "正在连接 Steam"],
    ["[2026-09-20 15:57:53] Loading Steam API...", "正在初始化 Steam"],
    ["[2026-09-20 15:37:16] Waiting for client config...", "正在获取 Steam 配置"],
    ["Waiting for user info...", "正在获取账户信息"],
    ["[2026-09-20 15:58:34] app_update 2857200", "正在获取游戏信息"],
    ["Update state (0x11) preallocating, progress: 0.00 (0 / 100)", "正在分配磁盘空间"],
    ["SteamCMD: Checking SteamCMD updates...", "正在检查 SteamCMD"],
    ["SteamCMD reported missing configuration. Retrying once...", "正在重试安装"],
  ]) {
    const html = render(job({ detail, install_progress: { phase: "preparing", percent: null } }));
    assert.match(html.replace(/<[^>]*>/g, ""), new RegExp(expected));
    assert.doesNotMatch(html, /aria-valuenow=|aria-label="下载速度:/);
  }
});

test("real download progress appears in the shared footer with concise text and diagnostics on hover", () => {
  const html = render(job());
  assert.match(html, /class="shell-task-activity is-active"/);
  assert.match(html, /role="progressbar"[^>]*aria-valuenow="50"/);
  assert.match(html, /512 KB \/ 1 MB/);
  assert.match(html, /aria-label="下载速度: —"/);
  assert.match(html.replace(/<[^>]*>/g, ""), /Game：正在下载/);
  assert.doesNotMatch(html.replace(/<[^>]*>/g, ""), /Raw diagnostic|Console output/);
  assert.match(render(job(), EN_US_EXTRA_MESSAGES), /Game: Downloading/);
});

test("retry context remains visible through later connection and download steps", () => {
  const waiting = job({ detail: "Retry 1: [2026-09-20 15:58:33] Waiting for client config...", install_progress: { phase: "preparing", percent: null } });
  assert.match(render(waiting).replace(/<[^>]*>/g, ""), /重试 1 · 正在获取 Steam 配置/);
  const downloading = job({ detail: "Retry 1: Update state (0x61) downloading, progress: 50.0 (512 / 1024)" });
  assert.match(render(downloading).replace(/<[^>]*>/g, ""), /重试 1 · 正在下载/);
  assert.match(render(downloading), /aria-valuenow="50"/);
  assert.match(render(waiting, EN_US_EXTRA_MESSAGES).replace(/<[^>]*>/g, ""), /Retry 1 · Fetching Steam configuration/);
});

test("unknown stages never use the old estimated job percentage and validation never claims a network speed", () => {
  for (const progress of [null, { phase: "preparing", percent: null }, { phase: "downloading", total_bytes: null, percent: null }]) {
    const value = job({ install_progress: progress });
    assert.equal(installationJobPercent(value), null);
    assert.doesNotMatch(render(value), /aria-valuenow=/);
  }
  const html = render(job({ install_progress: { phase: "verifying", percent: 72, downloaded_bytes: 1000, total_bytes: 2000, elapsed_seconds: 12 } }));
  assert.match(html, /aria-valuenow="72"/);
  assert.doesNotMatch(html, /aria-label="下载速度:|KB\/s|1 KB \/ 2 KB/);
});

test("recovered installations report their terminal result exactly on the active-to-terminal transition", () => {
  const previous = job();
  for (const status of ["Completed", "Failed", "Cancelled"]) {
    const terminal = job({ status });
    assert.equal(completedInstallationJob([previous], [terminal]), terminal);
    assert.equal(completedInstallationJob([terminal], [terminal]), null);
  }
  assert.equal(completedInstallationJob([previous], [job({ id: "another", status: "Completed" })]), null);
});

test("installation progress has one global footer surface and no cover banner", () => {
  const catalog = fs.readFileSync(require.resolve("../src/views/library/LibraryCatalogPage.tsx"), "utf8");
  assert.doesNotMatch(catalog, /library-job-banner|activeJob/);
  const shell = fs.readFileSync(require.resolve("../src/components/AppShell.tsx"), "utf8");
  assert.match(shell, /<footer[\s\S]*<InstallationActivity[\s\S]*<\/footer>/);
});
