import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ManualPlayerActions } from "../../src/views/servers/player-center/ManualPlayerActions";
import type { ExecuteInstanceManualPlayerActionInput, ExecuteInstancePlayerActionResult, ModulePlayerActionDetails } from "../../src/types";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, isTauri: true });
localStorage.setItem("langame.locale", "en-US");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
let checks = 0;
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
const requests: { input: ExecuteInstanceManualPlayerActionInput; result: ReturnType<typeof deferred<ExecuteInstancePlayerActionResult>> }[] = [];
mockIPC((command, args) => {
  check(command === "execute_instance_manual_player_action", `Unexpected native command: ${command}`);
  check(args && typeof args === "object" && "input" in args, "Action input is missing");
  const input = args.input as ExecuteInstanceManualPlayerActionInput;
  const result = deferred<ExecuteInstancePlayerActionResult>();
  requests.push({ input, result });
  return result.promise;
});
const kick: ModulePlayerActionDetails = {
  id: "kick_manual", label: "Kick", command_template: "kick {{target}}", target_required: true,
  target_label: "Player ID", destructive: true
};
const role: ModulePlayerActionDetails = {
  id: "set_role", label: "Assign role", command_template: "role {{target}} {{role}}", target_required: true,
  role_values: ["admin", "operator"], destructive: true
};
const root = createRoot(select<HTMLDivElement>("#fixture"));
function Fixture({ actions, disabled, moduleId }: { actions: ModulePlayerActionDetails[]; disabled: boolean; moduleId: string }) {
  const { t } = useI18n();
  return <ManualPlayerActions moduleId={moduleId} actions={actions} disabled={disabled} instanceId="manual-fixture" locale="en-US" t={t} />;
}
async function render(actions: ModulePlayerActionDetails[] = [kick], disabled = false, moduleId = "fixture") {
  await act(async () => {
    root.render(<StrictMode><I18nProvider><Fixture actions={actions} disabled={disabled} moduleId={moduleId} /></I18nProvider></StrictMode>);
  });
  const deadline = performance.now() + 5000;
  while (!document.querySelector(".player-center-manual-actions")) {
    check(performance.now() < deadline, "Manual player controls did not mount");
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) { await act(async () => { select<HTMLElement>(selector).click(); }); }
async function input(value: string) {
  await act(async () => {
    const target = select<HTMLInputElement>("input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function choose(selector: string, value: string) {
  await act(async () => {
    const element = select<HTMLSelectElement>(selector);
    element.value = value;
    element.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
const draft = () => select<HTMLInputElement>("input");
const feedback = () => document.querySelector<HTMLElement>(".shell-activity-notice");
async function run() {
  await act(async () => { await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]); });
  await render();
  check(!select<HTMLDetailsElement>("details.player-center-manual-actions").open, "Manual controls must start collapsed");
  await click("summary");
  check(select<HTMLDetailsElement>("details").open && !document.querySelector("select"), "A single action should not require an action dropdown");
  check(document.querySelectorAll("button").length === 1, "A single action repeated its execution button");
  checks++;

  await input("offline-player-42");
  await click("summary");
  check(!select<HTMLDetailsElement>("details").open, "Manual controls did not collapse");
  await click("summary");
  check(draft().value === "offline-player-42", "Collapsing manual controls discarded the draft");
  await render([{ ...kick }]);
  check(draft().value === "offline-player-42", "Refreshing action metadata discarded an in-progress draft");
  checks++;

  await click("button.danger");
  check(requests.length === 0 && select(".inline-confirm-message").textContent?.includes("offline-player-42"), "Destructive action bypassed exact-target review");
  await input("offline-player-99");
  check(!document.querySelector(".inline-confirm-review"), "Changing the target retained a stale confirmation");
  await click("button.danger");
  await click(".inline-confirm-submit");
  check(requests.length === 1 && requests[0].input.target === "offline-player-99"
    && requests[0].input.action_id === kick.id && requests[0].input.instance_id === "manual-fixture"
    && requests[0].input.role === null, "Reviewed action dispatched the wrong target or scope");
  checks++;

  await render([{ ...kick }]);
  check(draft().disabled && draft().value === "offline-player-99", "Action metadata refresh unlocked a pending action or cleared its target");
  await click("summary");
  await click("summary");
  check(draft().disabled, "Collapsing a pending action unlocked its form");
  await act(async () => { requests[0].result.reject(new Error("Exact player was not found")); });
  check(!draft().disabled && feedback()?.textContent === "Exact player was not found", "Pending action failure did not restore the draft and feedback");
  checks++;

  await render([kick, role]);
  check(draft().value === "offline-player-99", "Adding an available action discarded the existing draft");
  await choose("select[id^=manual-player-action]", role.id);
  check(draft().value === "", "Switching action types retained a potentially incompatible target");
  await input("operator-7");
  await choose("select[id^=manual-player-role]", "operator");
  await render([{ ...kick }, { ...role, role_values: [...role.role_values!] }]);
  check(select<HTMLSelectElement>("select[id^=manual-player-action]").value === role.id
    && select<HTMLSelectElement>("select[id^=manual-player-role]").value === "operator" && draft().value === "operator-7",
  "Action metadata refresh lost the selected action, role or target");
  checks++;

  await click("button.danger");
  await choose("select[id^=manual-player-role]", "admin");
  check(!document.querySelector(".inline-confirm-review"), "Role change retained a stale confirmation");
  await click("button.danger");
  await click(".inline-confirm-submit");
  check(requests.length === 2 && requests[1].input.action_id === role.id && requests[1].input.role === "admin"
    && requests[1].input.target === "operator-7", "Manual action did not retain the reviewed role and identity");
  await act(async () => { requests[1].result.resolve({ action_id: role.id, status: "sent", executed_at_unix_ms: 2, summary: "sent" }); });
  check(feedback()?.getAttribute("role") === "status" && !draft().disabled, "Manual action success did not unlock the form or report status");
  checks++;

  await render([{ ...role, role_values: ["operator"] }]);
  check(!document.querySelector("select") && select(".player-center-manual-body").textContent?.includes("operator"),
    "A single supported role should remain visible without a redundant dropdown");
  await input("sole-role-player");
  await click("button.danger");
  check(requests.length === 2 && select(".inline-confirm-message").textContent?.includes("sole-role-player"),
    "The single-role action bypassed exact-target confirmation");
  await click(".inline-confirm-submit");
  check(requests.length === 3 && requests[2].input.action_id === role.id && requests[2].input.role === "operator"
    && requests[2].input.target === "sole-role-player", "The single-role action lost its declared role or reviewed target");
  await act(async () => { requests[2].result.resolve({ action_id: role.id, status: "sent", executed_at_unix_ms: 3, summary: "sent" }); });
  checks++;

  await render([kick]);
  check(draft().value === "" && !document.querySelector("select"), "Removing the selected action retained its incompatible draft or dropdown");
  await render([kick], true);
  check(draft().disabled && select<HTMLButtonElement>("button.danger").disabled, "A stopped instance still offered a manual action");
  checks++;

  const humanitz = { ...kick, id: "kick_player" };
  await render([humanitz], false, "humanitz");
  const full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
  for (const value of ["76561198000000001", "FEDCBA9876543210fedcba9876543210", "Host_Offline", "0123|4567"]) {
    await input(value);
    check(select<HTMLButtonElement>("button.danger").disabled && draft().getAttribute("aria-invalid") === "true",
      "HumanitZ manual action accepted an ambiguous short identity");
    check(document.querySelector("[role=alert]")?.textContent?.includes("complete NetID"), "Missing NetID format feedback");
    await click("button.danger");
    check(requests.length === 3, "Invalid HumanitZ target reached IPC");
  }
  for (const value of [full, "|FEDCBA9876543210fedcba9876543210"]) {
    await input(value);
    check(!select<HTMLButtonElement>("button.danger").disabled, "Complete HumanitZ NetID is not usable");
    await click("button.danger");
    await click(".inline-confirm-submit");
    const request = requests[requests.length - 1];
    check(request.input.target === value, "HumanitZ target changed at dispatch");
    await act(async () => { request.result.resolve({ action_id: humanitz.id, status: "sent", executed_at_unix_ms: 4, summary: "sent" }); });
  }
  check(requests.length === 5, "Complete HumanitZ targets were not dispatched exactly once each");
  checks++;

  await act(async () => { root.unmount(); });
  clearMocks();
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Manual player fixture stalled after ${checks} checks`)), 20_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
