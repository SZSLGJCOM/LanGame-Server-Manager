import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppAiConnectionCheck } from "../../src/components/AppAiConnectionCheck";
import { AppAiSettingsCard } from "../../src/components/AppAiSettingsCard";
import { createDefaultAiSettings, type AiSettings } from "../../src/ai-settings";
import type { AssistantConnectionCheckOutput } from "../../src/assistant-connection-types";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import "../../src/app.css";
import "../../src/components/app-ai-settings.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, isTauri: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let switchLocale: ((locale: LocaleCode) => void) | null = null;
interface CheckInput { requestId: string; settings: Pick<AiSettings, "provider" | "model" | "baseUrl" | "apiKey"> }
interface PendingCheck { input: CheckInput; resolve: (value: unknown) => void; reject: (error: Error) => void }
const pending: PendingCheck[] = [];
const cancellations: Array<{ id: string; resolve: (value: boolean) => void; reject: (error: Error) => void }> = [];
Object.assign(window, { __TAURI_INTERNALS__: { invoke: (command: string, args: Record<string, unknown>) => {
  if (command === "assistant_check_connection") return new Promise((resolve, reject) => {
    pending.push({ input: args.input as CheckInput, resolve, reject });
  });
  if (command === "assistant_cancel_connection_check") return new Promise((resolve, reject) => {
    cancellations.push({ id: args.requestId as string, resolve, reject });
  });
  throw new Error(`Unexpected IPC command: ${command}`);
} } });
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
const fixture = document.getElementById("fixture")!;
let root = createRoot(fixture);
let current: AiSettings = { ...createDefaultAiSettings(), model: "fixture-model", apiKeyStored: true };
let persisted: AiSettings | undefined;
let disabled = false;
function Harness() {
  switchLocale = useI18n().setLocale;
  return <AppAiConnectionCheck settings={current} persistedSettings={persisted} disabled={disabled} />;
}
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) { check(performance.now() < deadline, message); await act(pause); }
}
async function render() {
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); });
  await settle(() => Boolean(fixture.querySelector(".ai-connection-check")), "Connection check did not mount");
}
const text = () => fixture.textContent ?? "";
const startButton = () => fixture.querySelector<HTMLButtonElement>(".ai-connection-actions button")!;
const cancelButton = () => fixture.querySelectorAll<HTMLButtonElement>(".ai-connection-actions button")[1];
const busy = () => fixture.querySelector(".ai-connection-check")?.getAttribute("aria-busy") === "true";
async function click(target: HTMLElement) { await act(async () => { target.click(); }); }
function output(entry = pending.at(-1)!, overrides: Partial<AssistantConnectionCheckOutput> = {}): AssistantConnectionCheckOutput {
  return { requestId: entry.input.requestId, provider: entry.input.settings.provider, model: entry.input.settings.model,
    endpointUrl: entry.input.settings.baseUrl, chat: { status: "passed", diagnostic: null, latencyMs: 100 },
    toolCall: { status: "passed", diagnostic: null, latencyMs: 150 }, toolReplay: { status: "passed", diagnostic: null, latencyMs: 200 },
    elapsedMs: 450, requestCount: 3, cancelled: false, ...overrides };
}
async function finish(overrides: Partial<AssistantConnectionCheckOutput> = {}) {
  await act(async () => { const entry = pending.at(-1)!; entry.resolve(output(entry, overrides)); });
}
async function start() {
  const previous = pending.length;
  await click(startButton());
  check(pending.length === previous + 1 && busy() && startButton().disabled, "Start did not reserve exactly one pending check");
}
async function finishCancelled() {
  await act(async () => { cancellations.at(-1)!.resolve(true); });
  check(busy(), "Cancellation acknowledgement released the still-running request");
  await finish();
  await settle(() => !busy(), "Cancelled check did not settle");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  current = { ...current, model: "", apiKeyStored: false };
  await render();
  check(startButton().disabled && text().includes("请先填写") && pending.length === 0, "Missing settings started a request or lacked feedback"); checks++;
  current = { ...current, model: "fixture-model", apiKeyStored: true };
  await render();
  check(!startButton().disabled && pending.length === 0 && text().includes("模型服务可能计费"), "Mount automatically tested the service or omitted request disclosure"); checks++;
  await act(async () => { startButton().click(); startButton().click(); });
  check(pending.length === 1 && startButton().disabled && text().includes("90 秒"), "Repeated clicks started duplicate checks");
  check(Object.keys(pending[0].input).sort().join(",") === "requestId,settings" && Object.keys(pending[0].input.settings).sort().join(",") === "apiKey,baseUrl,model,provider", "Connection test sent server or conversation data"); checks++;
  await click(cancelButton());
  check(cancellations.length === 1 && cancellations[0].id === pending[0].input.requestId && cancelButton().disabled, "Cancellation was not bound to the active request");
  await finish();
  check(busy() && startButton().disabled && !fixture.querySelector(".ai-connection-stages"), "Late success displayed after cancellation or released pending cancellation");
  await act(async () => { cancellations[0].resolve(true); });
  await settle(() => !busy(), "Cancellation did not release completed work");
  check(text().includes("检测已取消") && !startButton().disabled, "Completed cancellation lacks feedback or leaves controls locked"); checks++;
  await start();
  await finish();
  check(fixture.querySelectorAll(".ai-connection-stages .is-passed").length === 3 && text().includes("聊天和工具通信均已通过") && text().includes("3 次模型请求"), "Success did not expose the three tested stages"); checks++;

  const edits: Array<Partial<AiSettings>> = [
    { model: "changed-model" }, { baseUrl: "https://other.example.invalid/v1" }, { apiKey: ["synthetic", "changed", "key"].join("-") },
    { provider: "anthropic-compatible" }, { apiKey: "", apiKeyStored: false }
  ];
  for (const edit of edits) {
    current = { ...current, apiKeyStored: true };
    await render();
    await start();
    const before = cancellations.length;
    current = { ...current, ...edit };
    await render();
    check(cancellations.length === before + 1 && cancellations.at(-1)!.id === pending.at(-1)!.input.requestId && busy(), "Configuration edit did not cancel its exact pending request");
    await finishCancelled();
    check(!fixture.querySelector(".ai-connection-stages") && !text().includes("聊天和工具通信均已通过"), "Old results survived a configuration edit"); checks++;
  }
  current = { ...current, apiKeyStored: true };
  persisted = { ...current };
  await render();
  await start();
  persisted = { ...current, apiKeyStored: false };
  await render();
  await finishCancelled();
  check(!fixture.querySelector(".ai-connection-stages"), "External key clear allowed a success for the old saved credential"); checks++;
  await start();
  disabled = true;
  await render();
  await finishCancelled();
  check(startButton().disabled && !fixture.querySelector(".ai-connection-stages"), "Parent credential operation or Ollama loading left a check usable"); checks++;
  disabled = false;
  await render();

  await start();
  await finish({ toolCall: { status: "failed", diagnostic: "tool_not_called", latencyMs: 150 }, toolReplay: { status: "skipped", diagnostic: "tool_call_not_ready", latencyMs: 0 }, requestCount: 2 });
  check(text().includes("聊天可用，工具通信尚未通过") && text().includes("请确认它支持工具调用") && fixture.querySelectorAll(".is-skipped").length === 1, "Chat-only service was incorrectly described as ready for operations"); checks++;
  await start();
  await finish({ chat: { status: "failed", diagnostic: "authentication_rejected", latencyMs: 20 }, toolCall: { status: "skipped", diagnostic: "chat_not_ready", latencyMs: 0 }, toolReplay: { status: "skipped", diagnostic: "chat_not_ready", latencyMs: 0 }, requestCount: 1 });
  check(text().includes("服务拒绝了密钥") && !text().includes("聊天可用"), "Authentication failure did not remain a failed chat connection"); checks++;
  await start();
  await finish({ toolReplay: { status: "failed", diagnostic: '<img src=x onerror="unwanted=true">secret-payload-marker', latencyMs: 50 } });
  check(!text().includes("secret-payload-marker") && !fixture.querySelector("img"), "Unknown provider diagnostics escaped the fixed-message boundary"); checks++;
  await start();
  await act(async () => { pending.at(-1)!.reject(new Error("secret-payload-marker")); });
  check(text().includes("检测未完成") && !text().includes("secret-payload-marker") && !busy(), "Raw model error was exposed or the failure left controls locked"); checks++;
  await start();
  await click(cancelButton());
  await act(async () => { cancellations.at(-1)!.reject(new Error("secret-payload-marker")); });
  check(busy() && startButton().disabled, "Rejected stop request released active model work");
  await finish();
  check(text().includes("停止请求未获确认") && !text().includes("secret-payload-marker") && !busy(), "Unconfirmed cancellation was reported as a confirmed stop"); checks++;
  await start();
  await finish({ cancelled: true });
  check(text().includes("检测已取消") && !fixture.querySelector(".ai-connection-stages"), "Backend cancellation appeared as a passed check"); checks++;

  await start();
  const unmountRequest = pending.at(-1)!.input.requestId;
  await act(async () => { root.unmount(); });
  check(cancellations.at(-1)!.id === unmountRequest, "Closing settings did not cancel the owned request");
  await act(async () => { cancellations.at(-1)!.resolve(true); pending.at(-1)!.resolve(output()); });
  check(fixture.childElementCount === 0, "Unmounted request republished late output"); checks++;
  root = createRoot(fixture);
  await render();
  await act(async () => { switchLocale!("en-US"); });
  await settle(() => startButton()?.textContent?.includes("Check connection") === true, "English language did not load");
  await start();
  await finish();
  check(text().includes("Chat and tool communication passed") && text().includes("Tool result replay") && !text().includes("聊天"), "Connection results did not switch to English"); checks++;
  check(fixture.scrollWidth <= fixture.clientWidth + 1 && [...fixture.querySelectorAll("button")].every((entry) => entry.scrollWidth <= entry.clientWidth + 1), "Connection actions overflow the narrow settings panel"); checks++;

  await act(async () => { root.unmount(); });
  root = createRoot(fixture);
  Object.assign(globalThis, { isTauri: false });
  await render();
  check(startButton().disabled && text().includes("desktop app"), "Preview without a management host offered a fake connection test"); checks++;
  await act(async () => { root.unmount(); });
  root = createRoot(fixture);
  Object.assign(globalThis, { isTauri: true });
  const parentSettings = { ...createDefaultAiSettings(), model: "deepseek-chat", apiKeyStored: true };
  let acknowledgeSave: (() => void) | undefined;
  function ParentSettingsHarness() {
    const [settings, setSettings] = React.useState(parentSettings);
    const save = React.useCallback((draft: AiSettings) => new Promise<AiSettings>((resolve) => {
      acknowledgeSave = () => { setSettings(draft); resolve(draft); };
    }), []);
    return <AppAiSettingsCard settings={settings} onSave={save}
      onClearSecret={async (draft) => ({ ...draft, apiKeyStored: false })} />;
  }
  await act(async () => { root.render(<StrictMode><I18nProvider><ParentSettingsHarness /></I18nProvider></StrictMode>); });
  await settle(() => Boolean(fixture.querySelector("#ai-settings-model")), "Real AI settings card did not mount");
  await act(async () => {
    const input = fixture.querySelector<HTMLInputElement>("#ai-settings-model")!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "saved-fixture-model");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  check(startButton().disabled, "Connection check started before its model configuration was saved");
  await settle(() => Boolean(acknowledgeSave), "Parent autosave did not start");
  check(startButton().disabled, "Connection check started during the pending credential/configuration write");
  await act(async () => { acknowledgeSave!(); });
  await settle(() => !startButton().disabled, "Saved configuration did not enable the connection check"); checks++;
  await start();
  await finish();
  check(text().includes("Chat and tool communication passed") && fixture.querySelector("#ai-settings-model") && !startButton().disabled,
    "Connection check was not integrated into the real settings card or left its action locked"); checks++;
  fixture.querySelector(".ai-connection-check")!.scrollIntoView({ block: "center" });
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  return { status: "passed", checks, requests: pending.length, cancellations: cancellations.length, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Connection interaction stalled after ${checks} checks`)), 25000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
