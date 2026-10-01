import { act, StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { AssistantPanel } from "../../src/components/AssistantPanel";
import { createDefaultAiSettings, type AiSettings } from "../../src/ai-settings";
import { fallbackBootstrap } from "../../src/app-state";
import type { AssistantBuildInput } from "../../src/assistant-types";
import privacyDocument from "../../../../PRIVACY.md?raw";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const originalFetch = globalThis.fetch.bind(globalThis);
let externalRequests = 0;
globalThis.fetch = (resource, options) => {
  const url = new URL(resource instanceof Request ? resource.url : String(resource), location.href);
  if (url.origin !== location.origin) {
    externalRequests++;
    return Promise.reject(new Error("External network is disabled in this fixture"));
  }
  return originalFetch(resource, options);
};
let sends = 0;
let checks = 0;
const noop = () => {};
const fixtureRecipientUrl = new URL("https://models.example:8443/private-path/v1?key=private-key#private-fragment");
fixtureRecipientUrl.username = "private-user";
fixtureRecipientUrl.password = "fixture-password";
const settings: AiSettings = { ...createDefaultAiSettings(), enabled: true, model: "privacy-fixture", apiKeyStored: true,
  baseUrl: fixtureRecipientUrl.href };
const context: AssistantBuildInput = {
  aiSettings: settings, locale: "zh-CN", activeJobsCount: 0, activeView: "system",
  bootstrap: fallbackBootstrap, storageReady: true, libraryPage: "catalog", overlayNames: [],
  runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null,
  selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
  selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
  selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null,
};

function Harness({ aiSettings }: { aiSettings: AiSettings }) {
  const [draft, setDraft] = useState("保留这条尚未发送的消息");
  const panelRef = useRef<HTMLDivElement | null>(null);
  return <AssistantPanel aiSettings={aiSettings}
    assistantInput={context} draft={draft} execution={{ status: "idle", promptLabel: null, result: null, error: null }}
    panelRef={panelRef} messages={[]} conversations={[]} activeConversationId={null}
    onAction={noop} onClearAiSecret={async (next) => next} onClose={noop}
    onDeleteConversation={noop} onDraftChange={setDraft} onNewConversation={noop} onSelectConversation={noop}
    onRunPrompt={noop} onReady={noop} onSaveAiSettings={async (next) => next}
    onSendMessage={() => { sends++; }} />;
}
function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = document.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}`);
  return result;
}
function visible(selector: string) {
  const target = element(selector);
  const box = target.getBoundingClientRect();
  check(box.width > 0 && box.height > 0 && box.top >= 0 && box.bottom <= innerHeight
    && box.left >= 0 && box.right <= innerWidth, `${selector} is clipped: ${JSON.stringify(box.toJSON())}`);
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  check(hit && (target.contains(hit) || hit.contains(target)), `${selector} is covered`);
}
async function enter(target: HTMLElement) {
  target.focus();
  check(document.activeElement === target, "Control cannot receive keyboard focus");
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter failed");
  });
}
async function settle() {
  await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
}
async function scrollPolicyToEnd(body: HTMLElement) {
  body.focus();
  check(document.activeElement === body, "Privacy text cannot receive keyboard focus");
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/End`, { method: "POST" });
    check(response.ok, "Native End failed");
  });
  const deadline = performance.now() + 3000;
  while (body.scrollTop + body.clientHeight < body.scrollHeight - 2) {
    if (performance.now() > deadline) throw new Error("Keyboard could not reach the end of the policy");
    await settle();
  }
  check(body.scrollTop > 0, "Privacy text did not scroll");
}
const root = createRoot(element("#fixture"));
async function render(locale: string, aiSettings = settings) {
  localStorage.setItem("langame.locale", locale);
  await act(async () => { root.render(<StrictMode><I18nProvider key={locale}><Harness aiSettings={aiSettings} /></I18nProvider></StrictMode>); });
  const deadline = performance.now() + 5000;
  while (!document.querySelector(".assistant-chat-composer")) {
    if (performance.now() > deadline) throw new Error(`Composer did not load: ${errors.join("; ")}`);
    await settle();
  }
}
async function run() {
  for (const locale of ["zh-CN", "en-US"]) {
    await render(locale);
    check(document.querySelector(".ai-data-disclosure") === null, "Recipient disclosure must live in AI settings, outside the conversation");
    visible(".assistant-chat-input");
    visible(".assistant-privacy-button");
    const privacyLabel = element(".assistant-privacy-button").getAttribute("aria-label");
    check(privacyLabel === (locale === "zh-CN" ? "隐私与数据使用说明" : "Privacy and data use"), "Header privacy entry has the wrong language");
    await act(async () => { element(".assistant-history-button").click(); });
    check(document.querySelector(".assistant-history-drawer") !== null, "History did not open before privacy navigation");
    visible(".assistant-privacy-button");
    await enter(element(".assistant-privacy-button"));
    check(document.querySelector(".assistant-history-drawer") === null, "Entering privacy left the history drawer open");
    check(document.querySelector(".assistant-chat-composer") === null, "Privacy must open as a separate LAN surface");
    check(element(".assistant-panel").getAttribute("aria-label") === privacyLabel, "LAN dialog did not adopt the privacy page title");
    check(element(".assistant-panel-title").textContent === privacyLabel, "Privacy page title is missing");
    const body = element(".assistant-privacy-surface");
    check(body.querySelector("details, summary") === null, "Privacy page requires an unnecessary second disclosure click");
    check(body.querySelectorAll("h3").length >= 8, "Offline policy is incomplete");
    const language = locale === "zh-CN" ? "简体中文" : "English";
    const canonical = privacyDocument.split(`## ${language}`)[1].split(/\r?\n## /)[0];
    const titles = [...canonical.matchAll(/^### (.+)$/gm)].map((match) => match[1].trim());
    check([...body.querySelectorAll("h3")].map((heading) => heading.textContent).join("|") === titles.join("|"), "Wrong or incomplete policy language");
    check(body.scrollHeight > body.clientHeight && getComputedStyle(body).overflowY === "auto", "Privacy page needs its own bounded scroll area");
    check(body.scrollWidth <= body.clientWidth + 1 && document.documentElement.scrollWidth <= innerWidth,
      "Privacy page overflows horizontally");
    visible(".assistant-privacy-surface");
    visible(".assistant-back-button");
    visible(".assistant-close-button");
    await scrollPolicyToEnd(body);
    const lastParagraph = element(".assistant-privacy-surface section:last-child p:last-child");
    const lastBox = lastParagraph.getBoundingClientRect();
    const bodyBox = body.getBoundingClientRect();
    check(lastBox.bottom <= bodyBox.bottom && lastBox.bottom > bodyBox.top, "Last policy paragraph cannot be read in the page");
    await enter(element(".assistant-back-button"));
    check(document.querySelector(".assistant-privacy-surface") === null, "Back did not leave the privacy page");
    check(element(".assistant-history-button").getAttribute("aria-expanded") === "false", "Back reopened the history drawer");
    visible(".assistant-chat-input");
    visible(".assistant-send-control button");
    check(element<HTMLTextAreaElement>(".assistant-chat-input").value === "保留这条尚未发送的消息", "Reading policy changed the draft");
    await act(async () => { element(".assistant-settings-button").click(); });
    const keyNote = element("#ai-settings-key-note").textContent ?? "";
    check(keyNote.includes(locale === "zh-CN" ? "发送给配置的服务用于认证" : "sent to the configured service for authentication"), "Stored key hint hides credential transmission");
    const disclosure = element(".app-settings-card--ai .ai-data-disclosure");
    check(disclosure.textContent?.includes("https://models.example:8443"), "Settings did not show recipient");
    check(!/private-|fixture-password/.test(disclosure.textContent ?? ""), "Secret URL fields escaped into the disclosure");
    check(disclosure.textContent?.includes(locale === "zh-CN" ? "发送范围：" : "Sends conversation"), "Settings did not explain data sent to AI");
    check(disclosure.textContent?.includes(locale === "zh-CN" ? "接收方由服务地址决定" : "The service URL determines the recipient"), "Settings did not explain API protocol and recipient");
    disclosure.scrollIntoView({ block: "nearest" });
    await settle();
    visible(".app-settings-card--ai .ai-data-disclosure");
    check(document.querySelector(".app-settings-card--ai .ai-data-disclosure details, .app-settings-card--ai .privacy-notice-body") === null,
      "AI settings must use the shared header entry for the full policy");
    await enter(element(".assistant-back-button"));
    check(element<HTMLTextAreaElement>(".assistant-chat-input").value === "保留这条尚未发送的消息", "Settings navigation changed the draft");
    check(document.querySelector(".ai-data-disclosure") === null, "Returning to chat duplicated settings disclosure");
  }
  await render("zh-CN", { ...settings, provider: "ollama", baseUrl: "http://remote.example:11434/v1" });
  await enter(element(".assistant-settings-button"));
  const remoteRecipient = element(".ai-data-recipient").textContent ?? "";
  check(remoteRecipient.includes("http://remote.example:11434") && remoteRecipient.includes("HTTP 未加密")
    && !remoteRecipient.includes("管理端本机"), "Remote Ollama must not be described as local");
  await enter(element(".assistant-back-button"));
  await render("zh-CN", { ...settings, baseUrl: "broken-private-value" });
  await enter(element(".assistant-settings-button"));
  check(element(".ai-data-recipient").textContent?.includes("未设置有效")
    && !element(".ai-data-recipient").textContent?.includes("broken-private-value"), "Invalid recipient was disclosed or mislabeled");
  await enter(element(".assistant-back-button"));
  await render("zh-CN", { ...settings, baseUrl: "http://127.0.0.1:11434/v1" });
  await enter(element(".assistant-settings-button"));
  check(element(".ai-data-recipient").textContent?.includes("管理端本机"), "Loopback should refer to management host");
  await enter(element(".assistant-back-button"));
  const screenshotLocale = innerWidth < 1000 ? "en-US" : "zh-CN";
  await render(screenshotLocale, settings);
  await act(async () => { element(".assistant-privacy-button").click(); });
  visible(".assistant-privacy-surface");
  check(element(".assistant-privacy-surface").scrollTop === 0, "Reopening privacy must start at the beginning");
  check(sends === 0 && externalRequests === 0, "Reading notices triggered AI or external requests");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, sends, externalRequests, screenshotLocale, surface: "privacy", browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} checks`)), 20000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
