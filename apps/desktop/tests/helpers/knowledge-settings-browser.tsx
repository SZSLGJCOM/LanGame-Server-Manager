import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { KnowledgeSettingsCard } from "../../src/components/KnowledgeSettingsCard";
import type { KnowledgeApi, KnowledgeRuntimeStatus, KnowledgeSettings } from "../../src/knowledge-types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
let checks = 0;
let starts = 0;
let statusReads = 0;
const startedScopes: Array<string | null> = [];
let failSave = false;
let resolveLoad: (() => void) | null = null;
let resolveStart: (() => void) | null = null;
const initialLoad = new Promise<void>((resolve) => { resolveLoad = resolve; });
const cancellations: string[] = [];
const saved: KnowledgeSettings[] = [];
const model: KnowledgeRuntimeStatus = {
  library: { settings: { autoUpdate: true, intervalHours: 24 }, model: { id: "local-multilingual", revision: "fixture", ready: false, downloadBytes: 531_000_000 },
    games: [{ moduleId: "minecraft", scope: "Official server documentation", gaps: [], sources: [
      { id: "official-one", title: "Minecraft 官方开服指南", authority: "publisher", kind: "official", url: "https://example.invalid/server-docs",
        state: "ready", documentCount: 3, chunkCount: 12, lastCheckedAt: 1_750_000_100, lastSuccessAt: 1_750_000_000, lastError: null },
      { id: "official-two", title: "历史已收录的社区维护说明", authority: "wiki contributors", kind: "community", url: "https://example.invalid/maintenance",
        state: "error", documentCount: 1, chunkCount: 4, lastCheckedAt: 1_750_000_100, lastSuccessAt: 1_749_999_000, lastError: "官方站点暂时不可用，保留上次收录内容。" },
      { id: "restricted-source", title: "访问权限已改变的来源", authority: "publisher", kind: "official", url: "https://example.invalid/restricted",
        state: "restricted", documentCount: 2, chunkCount: 8, lastCheckedAt: 1_750_000_100, lastSuccessAt: 1_749_999_000, lastError: "Publisher documentation policy: Anonymous source is no longer public." }
    ] }, { moduleId: "palworld", scope: "Palworld dedicated servers", gaps: [], sources: [{ id: "palworld-docs", title: "Pocketpair server documentation", authority: "Pocketpair", kind: "official", url: "https://example.invalid/palworld", state: "pending", documentCount: 0, chunkCount: 0, lastCheckedAt: null, lastSuccessAt: null, lastError: null }] }], lastRun: null },
  job: { id: "bound-job", moduleId: null, state: "partial", startedAt: 1_750_000_000, finishedAt: 1_750_000_100,
    progress: { phase: "complete", moduleId: null, sourceId: null, completed: 2, total: 2, downloadedBytes: 0, totalDownloadBytes: 0 },
    report: { startedAt: 1_750_000_000, finishedAt: 1_750_000_100, changedDocuments: 1, sourcesSucceeded: 1, sourcesFailed: 2, cancelled: false, errors: ["Knowledge network request failed: HTTP 403", "Publisher documentation policy: Anonymous source is no longer public."] }, error: null }, schedulerError: null
};
const api: KnowledgeApi = {
  available: () => true,
  status: async () => { statusReads++; await initialLoad; return structuredClone(model); },
  save: async (settings) => { if (failSave) throw new Error("设置保存失败，请重试"); saved.push(settings); model.library.settings = { ...settings }; },
  start: async (moduleId) => { starts++; startedScopes.push(moduleId); await new Promise<void>((resolve) => { resolveStart = resolve; }); model.job = { ...model.job!, moduleId, id: "active-job-only", state: "running", finishedAt: null, report: null,
    progress: { phase: "model_download", moduleId: null, sourceId: null, completed: 0, total: 0, downloadedBytes: 100_000_000, totalDownloadBytes: 531_000_000 } }; return structuredClone(model.job); },
  cancel: async (id) => { cancellations.push(id); model.job!.state = "cancelling"; return true; }
};
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) { check(performance.now() < deadline, message); await act(pause); }
}
const text = () => document.getElementById("fixture")!.textContent ?? "";
const button = (label: string) => Array.from(document.querySelectorAll("button")).find((entry) => entry.textContent === label)!;
async function click(target: HTMLElement) { await act(async () => { target.click(); }); }
async function enter(target: HTMLElement) {
  target.focus();
  check(document.activeElement === target, "Knowledge detail control cannot receive keyboard focus");
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter failed for knowledge details");
  });
}

async function run() {
  const fixture = document.getElementById("fixture")!;
  const root = createRoot(fixture);
  await act(async () => { root.render(<StrictMode><I18nProvider><KnowledgeSettingsCard api={api} /></I18nProvider></StrictMode>); });
  await act(prepareBrowserLocaleCatalogs);
  await settle(() => Boolean(document.querySelector(".knowledge-settings")), "Language catalog did not mount the knowledge settings");
  check(text().includes("正在读取知识库"), "Initial asynchronous loading state is missing"); checks++;
  await act(async () => { resolveLoad!(); });
  await settle(() => text().includes("6 篇正文"), "Knowledge counts did not load");
  check(document.querySelector(".knowledge-settings-counts")!.textContent === "已缓存 6 篇正文" && text().includes("531 MB"), "Cached documents or first-download size are missing"); checks++;
  const updateDetails = document.querySelector<HTMLDetailsElement>(".knowledge-settings-update-details")!;
  const sourcesDetails = document.querySelector<HTMLDetailsElement>(".knowledge-settings-sources")!;
  const indexDetails = document.querySelector<HTMLDetailsElement>(".knowledge-settings-library-details")!;
  // Closed details use content-visibility in current Chromium; their contents
  // can still have layout rectangles without being visible to the user.
  check(!updateDetails.open && !sourcesDetails.open && !indexDetails.open
    && !document.querySelector(".knowledge-settings-diagnostics")!.checkVisibility({ visibilityProperty: true, opacityProperty: true })
    && !document.querySelector(".knowledge-settings-index-counts")!.checkVisibility({ visibilityProperty: true, opacityProperty: true }),
  "Low-frequency details or raw failures expanded on the initial page"); checks++;
  const summary = document.querySelector(".knowledge-settings-progress")!.textContent!;
  check(summary.includes("先前收录的正文与索引已保留") && summary.includes("部分来源未更新") && !summary.includes("仍可使用"), "Partial failure lost retained-cache evidence or claimed restricted content was usable"); checks++;
  await enter(updateDetails.querySelector("summary")!);
  check(updateDetails.open && document.querySelector(".knowledge-settings-diagnostics")!.checkVisibility({ visibilityProperty: true, opacityProperty: true })
    && updateDetails.textContent!.includes("HTTP 403") && updateDetails.textContent!.includes("Publisher documentation policy"), "Keyboard could not reveal full update diagnostics");
  const readsBeforeReload = statusReads;
  await click(button("重新读取状态"));
  check(statusReads > readsBeforeReload && starts === 0 && saved.length === 0, "Reload status unexpectedly started a sync or changed settings");
  await enter(updateDetails.querySelector("summary")!); checks++;
  await enter(indexDetails.querySelector("summary")!);
  check(indexDetails.open && indexDetails.textContent!.includes("2 个游戏资料目录") && indexDetails.textContent!.includes("24 个索引片段")
    && indexDetails.textContent!.includes("片段会随上下文发送") && indexDetails.textContent!.includes("受限来源不会提供给 LAN 检索"), "Index details lost accurate counts, transmission or restriction semantics");
  await enter(indexDetails.querySelector("summary")!); checks++;
  await enter(sourcesDetails.querySelector("summary")!);
  const restricted = Array.from(sourcesDetails.querySelectorAll<HTMLElement>(".knowledge-settings-source")).find((source) => source.textContent!.includes("访问权限已改变的来源"))!;
  check(sourcesDetails.open && sourcesDetails.textContent!.includes("最近检查") && sourcesDetails.textContent!.includes("最近成功")
    && sourcesDetails.textContent!.includes("官方发布") && sourcesDetails.textContent!.includes("社区维护")
    && sourcesDetails.textContent!.includes("更新失败") && restricted.textContent!.includes("来源受限") && restricted.textContent!.includes("已缓存 2 篇正文")
    && !restricted.querySelector(".knowledge-settings-source-error, .is-warning") && !restricted.textContent!.includes("仅参考链接")
    && sourcesDetails.querySelector<HTMLAnchorElement>("a")!.href === "https://example.invalid/server-docs",
  "Source details lost provenance, links, state distinctions or treated all restrictions as reference-only failures"); checks++;
  const game = document.querySelector<HTMLSelectElement>('select[aria-label="游戏资料"]')!;
  await act(async () => { game.value = "palworld"; game.dispatchEvent(new Event("change", { bubbles: true })); });
  check(text().includes("Pocketpair server documentation") && !text().includes("Minecraft 官方开服指南"), "Selecting a game did not filter source details"); checks++;
  const update = button("更新此游戏");
  await act(async () => { update.click(); update.click(); });
  check(starts === 1 && update.disabled && startedScopes[0] === "palworld", "Repeated update clicks started duplicate work or used the wrong game");
  await act(async () => { resolveStart!(); });
  await settle(() => Boolean(button("取消更新")), "Running job did not expose cancellation");
  check(text().includes("100 / 531 MB") && button("更新此游戏").disabled, "Model download progress is missing or update remains enabled"); checks++;
  await click(button("取消更新"));
  check(cancellations.length === 1 && cancellations[0] === "active-job-only", "Cancellation did not bind to the displayed job");
  check(button("取消更新").disabled && text().includes("正在取消并保留已发布索引"), "Cancellation was treated as completed before worker cleanup"); checks++;
  model.job!.state = "cancelled";
  model.job!.finishedAt = 1_750_000_200;
  await settle(() => text().includes("已取消更新"), "Cancelled worker was not refreshed");
  check(!button("更新此游戏").disabled && text().includes("已缓存 6 篇正文") && !button("取消更新"), "Cancellation lost old documents or kept stale controls"); checks++;
  await click(button("更新全部游戏"));
  await act(async () => { resolveStart!(); });
  await settle(() => Boolean(button("取消更新")), "All-game update did not start");
  check(startedScopes.length === 2 && startedScopes[1] === null && document.querySelector(".knowledge-settings-progress")!.textContent!.includes("全部游戏"), "Explicit all-game update retained a selected-game scope"); checks++;
  failSave = true;
  await click(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
  check(document.querySelector('[role="alert"]')?.textContent === "知识库操作未完成，请查看更新详情。"
    && updateDetails.textContent!.includes("设置保存失败") && document.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked,
  "Failed setting save lacks immediate feedback, lost diagnostics or changed persisted preference"); checks++;
  failSave = false;
  await click(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
  const interval = document.querySelector("select")!;
  await act(async () => { interval.value = "72"; interval.dispatchEvent(new Event("change", { bubbles: true })); });
  check(saved.length === 2 && saved[0].autoUpdate === false && saved[1].intervalHours === 72 && !saved[1].autoUpdate, "Settings did not preserve independent values across writes"); checks++;
  model.library.model.ready = true;
  model.job!.state = "completed";
  model.job!.finishedAt = 1_750_000_300;
  await settle(() => document.querySelector(".knowledge-settings-model-state")!.textContent!.includes("已就绪")
    && !button("取消更新"), "Completed update did not expose the ready model or release cancellation");
  await act(async () => { [updateDetails, sourcesDetails, indexDetails].forEach((details) => { details.open = false; }); });
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Knowledge settings overflow the narrow desktop panel");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Knowledge interaction stalled after ${checks} checks`)), 20000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
