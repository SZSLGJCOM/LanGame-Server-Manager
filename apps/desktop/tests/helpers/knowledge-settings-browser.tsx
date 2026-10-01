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
        state: "current", documentCount: 3, chunkCount: 12, lastCheckedAt: 1_750_000_100, lastSuccessAt: 1_750_000_000, lastError: null },
      { id: "official-two", title: "历史已收录的社区维护说明", authority: "wiki contributors", kind: "community", url: "https://example.invalid/maintenance",
        state: "failed", documentCount: 1, chunkCount: 4, lastCheckedAt: 1_750_000_100, lastSuccessAt: 1_749_999_000, lastError: "官方站点暂时不可用，保留上次收录内容。" }
    ] }, { moduleId: "palworld", scope: "Palworld dedicated servers", gaps: [], sources: [{ id: "palworld-docs", title: "Pocketpair server documentation", authority: "Pocketpair", kind: "official", url: "https://example.invalid/palworld", state: "pending", documentCount: 0, chunkCount: 0, lastCheckedAt: null, lastSuccessAt: null, lastError: null }] }], lastRun: null },
  job: { id: "bound-job", moduleId: null, state: "partial", startedAt: 1_750_000_000, finishedAt: 1_750_000_100,
    progress: { phase: "complete", moduleId: null, sourceId: null, completed: 2, total: 2, downloadedBytes: 0, totalDownloadBytes: 0 },
    report: { startedAt: 1_750_000_000, finishedAt: 1_750_000_100, changedDocuments: 1, sourcesSucceeded: 1, sourcesFailed: 1, cancelled: false, errors: ["官方站点暂时不可用"] }, error: null }, schedulerError: null
};
const api: KnowledgeApi = {
  available: () => true,
  status: async () => { await initialLoad; return structuredClone(model); },
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

async function run() {
  const fixture = document.getElementById("fixture")!;
  const root = createRoot(fixture);
  await act(async () => { root.render(<StrictMode><I18nProvider><KnowledgeSettingsCard api={api} /></I18nProvider></StrictMode>); });
  await act(prepareBrowserLocaleCatalogs);
  await settle(() => Boolean(document.querySelector(".knowledge-settings")), "Language catalog did not mount the knowledge settings");
  check(text().includes("正在读取知识库"), "Initial asynchronous loading state is missing"); checks++;
  await act(async () => { resolveLoad!(); });
  await settle(() => text().includes("4 篇正文"), "Knowledge counts did not load");
  check(text().includes("16 个索引片段") && text().includes("531 MB"), "Real model size or indexed counts are missing"); checks++;
  check(text().includes("已有正文和索引仍可使用") && text().includes("部分来源未更新"), "Partial failure falsely discarded or completed the old index"); checks++;
  await click(document.querySelector("summary")!);
  check(document.querySelector("details")!.open && text().includes("最近检查") && text().includes("最近成功") && text().includes("官方发布") && text().includes("社区维护") && text().includes("官方站点暂时不可用，保留上次收录内容。"), "Source provenance, coverage or historical success evidence is missing"); checks++;
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
  check(!button("更新此游戏").disabled && text().includes("4 篇正文") && !button("取消更新"), "Cancellation lost old documents or kept stale controls"); checks++;
  await click(button("更新全部游戏"));
  await act(async () => { resolveStart!(); });
  await settle(() => Boolean(button("取消更新")), "All-game update did not start");
  check(startedScopes.length === 2 && startedScopes[1] === null && document.querySelector(".knowledge-settings-progress")!.textContent!.includes("全部游戏"), "Explicit all-game update retained a selected-game scope"); checks++;
  failSave = true;
  await click(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
  check(text().includes("设置保存失败") && document.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked, "Failed setting save silently changed persisted preference"); checks++;
  failSave = false;
  await click(document.querySelector<HTMLInputElement>('input[type="checkbox"]')!);
  const interval = document.querySelector("select")!;
  await act(async () => { interval.value = "72"; interval.dispatchEvent(new Event("change", { bubbles: true })); });
  check(saved.length === 2 && saved[0].autoUpdate === false && saved[1].intervalHours === 72 && !saved[1].autoUpdate, "Settings did not preserve independent values across writes"); checks++;
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Assistant settings overflow the narrow desktop panel");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Knowledge interaction stalled after ${checks} checks`)), 20000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
