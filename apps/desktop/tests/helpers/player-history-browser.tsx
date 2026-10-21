import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, ModuleDetails, RuntimeLivePlayerSnapshot } from "../../src/types";
import type { PlayerHistoryEvent, PlayerHistoryPage, PlayerHistoryQuery } from "../../src/player-history";
import { PlayerCenterWorkbench } from "../../src/views/servers/PlayerCenterWorkbench";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let mode: "normal" | "empty" | "error" | "deferred" = "normal";
const requests: PlayerHistoryQuery[] = [];
const pending: { query: PlayerHistoryQuery; resolve: (result: PlayerHistoryPage) => void }[] = [];
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
const entries: PlayerHistoryEvent[] = [
  { id: 42, run_id: 10, process_key: "master", kind: "leave", player_name: "历史玩家", account_id: null,
    message: null, log_time: "00:10:00", estimated_at_unix_ms: 1_792_282_200_000, duration_ms: 540_000, session_ambiguous: false },
  { id: 41, run_id: 10, process_key: "volcano", kind: "chat", player_name: "历史玩家", account_id: "KU_history",
    message: "一起探索火山 <script>window.historyInjected = true</script>", log_time: "00:05:00", estimated_at_unix_ms: 1_792_281_900_000, duration_ms: null, session_ambiguous: false },
  { id: 40, run_id: 10, process_key: "master", kind: "join", player_name: "历史玩家", account_id: null,
    message: null, log_time: "00:01:00", estimated_at_unix_ms: 1_792_281_660_000, duration_ms: 540_000, session_ambiguous: false }
];
function page(query: PlayerHistoryQuery, events = entries): PlayerHistoryPage {
  const filtered = events.filter((event) => (!query.kind || event.kind === query.kind)
    && (!query.search || [event.player_name, event.account_id, event.message].some((value) => value?.includes(query.search!))));
  return { instance_id: query.instance_id, supported: true,
    events: query.before_id ? filtered.slice(2) : filtered.slice(0, 2),
    next_before_id: !query.before_id && filtered.length > 2 ? 41 : null, collection_issue: null };
}
function online(instanceId: string): RuntimeLivePlayerSnapshot {
  return { snapshot_id: `live-${instanceId}`, instance_id: instanceId, status: "ready", source: "runtime_action",
    observed_at_unix_ms: Date.now(), expires_at_unix_ms: Date.now() + 60_000, complete: true, truncated: false, stale: false,
    current_players: 1, max_players: 8, issue: null, entries: [{ player_key: "live-player", display_name: "当前玩家",
      identifiers: [{ kind: "klei_user_id", value: "KU_current", stable: true }], available_action_ids: ["kick_player", "ban_player"],
      ping_ms: 32, session_started_at_unix_ms: Date.now() - 60_000, role: null, attributes: [{ key: "prefab", value: "wilson" }] }] };
}
// Only native IPC is mocked; the real player workspace, history view, filtering
// controls, API envelope, request lifecycle and right-hand actions run unchanged.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => {
  if (command === "read_instance_live_players" || command === "refresh_instance_live_players") return Promise.resolve(online(String(args?.instanceId)));
  if (command !== "read_instance_player_history") return invokeMock(command, args);
  const query = args?.input as PlayerHistoryQuery;
  requests.push(query);
  if (mode === "deferred") return new Promise<PlayerHistoryPage>((resolve) => pending.push({ query, resolve }));
  if (mode === "error") return Promise.reject(new Error("History read failed"));
  return Promise.resolve(page(query, mode === "empty" ? [] : entries));
} } });
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node;
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) { if (performance.now() >= deadline) throw new Error(message); await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); }); }
}
async function click(text: string, scope = ".player-history") {
  const button = Array.from(element(scope).querySelectorAll<HTMLButtonElement>("button")).find((node) => node.textContent === text);
  check(button && !button.disabled, `Enabled ${text} button required`); await act(async () => button.click());
}
async function input(selector: string, value: string) {
  const node = element<HTMLInputElement | HTMLSelectElement>(selector);
  await act(async () => {
    const prototype = node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function render(instanceId: string) {
  details = { ...details, summary: { ...details.summary, id: instanceId } };
  await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 16, height: "100vh", boxSizing: "border-box" }}>
    <PlayerCenterWorkbench details={details} moduleDetails={moduleDetails} runtime={null} />
  </main></I18nProvider></StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".player-center-player-table tbody tr")), "Online list did not load");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((candidate) => candidate.module_id === "dontstarve")!;
  details = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });
  moduleDetails = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "dontstarve" });
  await render(instance.id);
  check(requests.length === 0, "History must not query or synthesize events while the history view is closed");
  await act(async () => element(".player-center-player-table tbody tr").click());
  const controls = element(".player-center-controls");
  const controlsText = controls.textContent;
  const bounds = controls.getBoundingClientRect();
  await click("历史记录", ".player-center-history-tabs");
  await settle(() => Boolean(fixture.querySelector(".player-history-table")), "History did not load");
  check(controls === element(".player-center-controls") && controls.textContent === controlsText, "History must preserve the mounted management panel and selected player");
  check(controls.getBoundingClientRect().width === bounds.width, "History must not resize the management panel");
  check(element(".player-history-table").textContent?.includes("0:09:00"), "Paired online duration must be visible");
  check(element(".player-history-table").textContent?.includes("KU_history"), "Native chat account must be visible");
  check(element(".player-history-table").textContent?.includes("<script>") && !fixture.querySelector(".player-history script"), "Chat must render as escaped text");
  check(requests.at(-1)?.limit === 50 && requests.at(-1)?.instance_id === instance.id, "History request must use bounded paging and current instance");
  await click("下一页"); await settle(() => Boolean(fixture.textContent?.includes("第 2 页")) && !element(".player-history").getAttribute("aria-busy")?.includes("true"), "Next page did not load");
  check(requests.at(-1)?.before_id === 41 && element(".player-history-table").textContent?.includes("进入"), "Next page must forward the returned cursor");
  await click("上一页"); await settle(() => Boolean(fixture.querySelector(".player-history-table")), "Previous page did not load");
  check(requests.at(-1)?.before_id === undefined, "Previous page must restore first-page cursor");
  await input('.player-history select', "chat"); await input('.player-history input[maxlength="256"]', "KU_history");
  await click("查询"); await settle(() => Boolean(fixture.querySelector(".player-history-table")), "Filtered history did not load");
  check(requests.at(-1)?.kind === "chat" && requests.at(-1)?.search === "KU_history", "Type and search filters must reach API");
  check(fixture.querySelectorAll(".player-history-table tbody tr").length === 1, "Filter results must replace the page");
  mode = "empty"; await click("刷新"); await settle(() => Boolean(fixture.textContent?.includes("没有符合条件")), "Empty state missing");
  mode = "error"; await click("刷新"); await settle(() => Boolean(fixture.textContent?.includes("History read failed")), "Error state missing");
  mode = "deferred"; await click("刷新");
  check(element(".player-history").getAttribute("aria-busy") === "true" && fixture.textContent?.includes("正在读取历史"), "Loading state must be explicit");
  await render("history-other");
  mode = "normal"; await click("历史记录", ".player-center-history-tabs");
  await settle(() => Boolean(fixture.querySelector(".player-history-table")), "Other instance history missing");
  await act(async () => { for (const task of pending) task.resolve(page(task.query, [{ ...entries[0], player_name: "OLD INSTANCE LEAK" }])); });
  check(!fixture.textContent?.includes("OLD INSTANCE LEAK") && requests.at(-1)?.instance_id === "history-other", "Late history must remain scoped to its instance");
  check(element<HTMLInputElement>('.player-history input[maxlength="256"]').value === "", "New instance must reset old filters");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Player history must not overflow the viewport");
  const results = element(".player-history-results").getBoundingClientRect();
  check(results.height > 80 && results.bottom <= innerHeight, "History must retain a usable scrolling content region");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, requests: requests.length, instance_isolation: true, management_preserved: true };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`History fixture stalled after ${checks} checks`)), 25000); })])
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
