import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { I18nProvider, useI18n } from "../../src/i18n";
import { LivePlayerActionPanel } from "../../src/views/servers/player-center/LivePlayerActionPanel";
import type { ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, isTauri: true });
localStorage.setItem("langame.locale", "en-US");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
let checks = 0;
let refreshCalls = 0;
let refreshFailure = false;
let pendingRefresh: ReturnType<typeof deferred<void>> | null = null;
let injectedExecutor: ((input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>) | undefined;
const requests: { input: ExecuteInstancePlayerActionInput; result: ReturnType<typeof deferred<ExecuteInstancePlayerActionResult>> }[] = [];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function select<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  check(element, `Missing element: ${selector}`);
  return element;
}
mockIPC((command, args) => {
  check(command === "execute_instance_player_action", `Unexpected native command: ${command}`);
  check(args && typeof args === "object" && "input" in args, "Action input is missing");
  const input = args.input as ExecuteInstancePlayerActionInput;
  check(typeof input.instance_id === "string" && typeof input.snapshot_id === "string"
    && typeof input.player_key === "string" && typeof input.action_id === "string", "Action input lost its snapshot identity");
  const result = deferred<ExecuteInstancePlayerActionResult>();
  requests.push({ input, result });
  return result.promise;
});
const players: RuntimeLivePlayerEntry[] = ["Alice", "Bob"].map((name) => ({
  player_key: name.toLowerCase(), display_name: name,
  identifiers: [{ kind: "player_id", value: name.toLowerCase(), stable: true }],
  available_action_ids: ["kick", "inspect"], ping_ms: 20, session_started_at_unix_ms: 1,
  role: null, attributes: []
}));
function snapshot(instance = "one", revision = "first"): RuntimeLivePlayerSnapshot {
  return { snapshot_id: revision, instance_id: instance, status: "ready", source: "runtime_action",
    observed_at_unix_ms: 1, expires_at_unix_ms: 10000, complete: true, truncated: false, stale: false,
    current_players: players.length, max_players: 20, entries: players, issue: null };
}
const root = createRoot(select<HTMLDivElement>("#fixture"));
function Fixture({ current, selected, actionsEnabled }: {
  current: RuntimeLivePlayerSnapshot; selected: RuntimeLivePlayerEntry | null; actionsEnabled: boolean
}) {
  const { t } = useI18n();
  return <LivePlayerActionPanel actionsEnabled={actionsEnabled} actionIds={["kick", "inspect"]} declaredActions={[
    { id: "kick", label: "Kick", command_template: "kick {{target}}", destructive: true },
    { id: "inspect", label: "Inspect", command_template: "inspect {{target}}", destructive: false }
  ]} locale="en-US" snapshot={current} player={selected} t={t} onExecute={injectedExecutor} onActionCompleted={async () => {
    refreshCalls++;
    if (refreshFailure) throw new Error("Refresh connection failed");
    if (pendingRefresh) await pendingRefresh.promise;
  }} />;
}
async function render(selected: RuntimeLivePlayerEntry | null = players[0], current = snapshot(), actionsEnabled = true) {
  await act(async () => {
    root.render(<StrictMode><I18nProvider><Fixture current={current} selected={selected} actionsEnabled={actionsEnabled} /></I18nProvider></StrictMode>);
  });
  const deadline = performance.now() + 5000;
  while (!document.querySelector(".player-center-member-pane")) {
    check(performance.now() < deadline, "Player action panel did not mount");
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) { await act(async () => { select<HTMLElement>(selector).click(); }); }
async function submitKick() {
  const count = requests.length;
  await click(".player-center-member-action.danger");
  check(requests.length === count, "Destructive action bypassed inline review");
  await click(".inline-confirm-submit");
  check(requests.length === count + 1, "Confirmed action did not reach the API boundary");
}
async function settle(index: number, error?: string) {
  await act(async () => {
    const request = requests[index];
    if (error) request.result.reject(new Error(error));
    else request.result.resolve({ action_id: request.input.action_id, status: "sent", executed_at_unix_ms: 2, summary: "sent" });
  });
}
const feedback = () => document.querySelector<HTMLElement>(".shell-activity-notice");
const inspect = () => select<HTMLButtonElement>("button.player-center-member-action:not(.danger)");
function verifyDisabledActions() {
  const actions = Array.from(document.querySelectorAll<HTMLButtonElement>(".player-center-member-action"));
  check(actions.length === 2 && actions.every((action) => action.disabled),
    "Unavailable online target must retain both disabled declared actions");
}

async function run() {
  await act(async () => { await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]); });
  await render(null); verifyDisabledActions();
  check(!document.querySelector(".player-center-member-prompt, .player-center-member-heading strong[title], .player-center-member-heading > span"),
    "Unselected online actions still show a prompt or a stale player header");
  await click(".player-center-member-action.danger"); await click("button.player-center-member-action:not(.danger)");
  check(requests.length === 0 && !document.querySelector(".inline-confirm-review"), "An unselected action reached review or dispatch"); checks++;
  await render({ ...players[0], available_action_ids: ["inspect"] });
  check(select<HTMLButtonElement>(".player-center-member-action.danger").disabled && !inspect().disabled,
    "Available actions on the selected row did not control each declared button independently");
  await click(".player-center-member-action.danger");
  check(requests.length === 0 && !document.querySelector(".inline-confirm-review"), "A row without kick permission opened destructive review"); checks++;
  await render(players[0], snapshot(), false); verifyDisabledActions();
  await click("button.player-center-member-action:not(.danger)");
  check(requests.length === 0, "An expired or unavailable snapshot dispatched an action"); checks++;
  await render();
  await submitKick();
  check(JSON.stringify(requests[0].input) === JSON.stringify({ instance_id: "one", snapshot_id: "first", player_key: "alice", action_id: "kick" }),
    "The confirmed action must keep its original snapshot and player target");
  await render(players[1]);
  await settle(0, "Alice action failed");
  check(!feedback(), "Alice's late error was shown in Bob's panel");
  check(refreshCalls === 0, "A rejected action must not refresh the player list");
  await render(players[0]);
  check(feedback()?.textContent === "Alice action failed", "Returning to Alice lost her action result");
  checks++;

  await submitKick();
  await render(players[0], snapshot("one", "second"));
  check(inspect().disabled, "A snapshot refresh released a still-pending action");
  await click("button.player-center-member-action:not(.danger)");
  check(requests.length === 2, "A refreshed snapshot permitted a duplicate action");
  await render(players[1], snapshot("one", "second"));
  check(!document.querySelector(".player-center-member-pane")?.textContent?.includes("Running..."), "Bob inherited Alice's running action label");
  check(inspect().disabled && !feedback(), "Selection change released the pending command or misattributed its feedback");
  await settle(1, "Original snapshot refused");
  check(!feedback(), "Snapshot refresh allowed Alice's error into Bob's panel");
  checks++;

  await render(players[0], snapshot("one", "second"));
  refreshFailure = true;
  await submitKick();
  await settle(2);
  check(feedback()?.textContent?.includes("Action sent") && feedback()?.textContent?.includes("Refresh connection failed"),
    "Refresh failure must preserve successful command delivery and explain the refresh error");
  check(refreshCalls === 1 && !inspect().disabled, "Refresh failure did not release the completed operation");
  refreshFailure = false;
  checks++;

  pendingRefresh = deferred<void>();
  await click("button.player-center-member-action:not(.danger)");
  await settle(3);
  check(inspect().disabled, "The action was released before its refresh finished");
  await render(players[0], snapshot("one", "third"));
  check(inspect().disabled, "Snapshot arrival during refresh permitted duplicate submission");
  await act(async () => { pendingRefresh!.resolve(); });
  pendingRefresh = null;
  check(!inspect().disabled && feedback()?.getAttribute("role") === "status", "Successful refresh lost delivery feedback or kept actions locked");
  checks++;

  await click("button.player-center-member-action:not(.danger)");
  const beforeInstanceChange = refreshCalls;
  await render(players[0], snapshot("two"));
  check(!inspect().disabled && !select<HTMLButtonElement>(".player-center-member-action.danger").disabled && !feedback(),
    "A new instance inherited a previous instance's pending state");
  await click("button.player-center-member-action:not(.danger)");
  await settle(4);
  check(refreshCalls === beforeInstanceChange && !feedback() && inspect().disabled,
    "Old instance completion refreshed or unlocked the new instance");
  await settle(5, "Second instance failed");
  check(feedback()?.textContent === "Second instance failed", "New instance result was lost");
  checks++;

  await click("button.player-center-member-action:not(.danger)");
  const beforeUnmount = refreshCalls;
  await act(async () => { root.render(null); });
  await render(players[0], snapshot("two"));
  await settle(6);
  check(!feedback() && !inspect().disabled && refreshCalls === beforeUnmount,
    "An unmounted request contaminated the remounted instance or refreshed its data");
  checks++;

  await act(async () => { inspect().click(); inspect().click(); });
  check(requests.length === 8, "Two same-tick clicks dispatched duplicate non-destructive actions");
  await settle(7);
  check(!inspect().disabled && feedback()?.getAttribute("role") === "status", "Successful command did not report success");
  checks++;

  await click(".player-center-member-action.danger");
  await render(players[1], snapshot("two"));
  check(!document.querySelector(".inline-confirm-review") && requests.length === 8,
    "Changing players preserved a stale destructive review");
  await click(".player-center-member-action.danger");
  await render(null, snapshot("two")); verifyDisabledActions();
  check(!document.querySelector(".inline-confirm-review, .player-center-member-heading strong[title], .player-center-member-heading > span") && requests.length === 8,
    "Clearing the player selection preserved a target or destructive review");
  await render(null, { ...snapshot("two", "empty"), entries: [], current_players: 0 }); verifyDisabledActions(); checks++;
  const injectedInputs: ExecuteInstancePlayerActionInput[] = [];
  const beforeInjectedRefresh = refreshCalls;
  injectedExecutor = async (input) => {
    injectedInputs.push(input);
    return { action_id: input.action_id, status: "sent", executed_at_unix_ms: 3, summary: "sent" };
  };
  await render(players[0], snapshot("two", "injected"));
  await click("button.player-center-member-action:not(.danger)");
  check(JSON.stringify(injectedInputs) === JSON.stringify([{ instance_id: "two", snapshot_id: "injected", player_key: "alice", action_id: "inspect" }])
    && requests.length === 8 && refreshCalls === beforeInjectedRefresh + 1,
    "Injected authoritative action handler was bypassed, duplicated, or lost the live-list refresh"); checks++;
  await act(async () => { root.unmount(); });
  clearMocks();
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Player action fixture stalled after ${checks} checks`)), 20_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
