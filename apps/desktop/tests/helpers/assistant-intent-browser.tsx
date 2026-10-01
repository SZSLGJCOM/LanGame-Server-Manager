import { act, StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { AssistantPanel } from "../../src/components/AssistantPanel";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { fallbackBootstrap } from "../../src/app-state";
import { assistantWorkflowResultMessage, runAssistantWorkflow } from "../../src/assistant-workflow";
import type { AssistantBuildInput } from "../../src/assistant-types";
import type { AssistantChatMessage, AssistantContinuation, AssistantExecuteOperationOutput, AssistantExecutionState } from "../../src/types";
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
let stopRequests = 0;
let resumeRequests = 0;
const submitted: { prompt: string; argumentCount: number }[] = [];
const settings = { ...createDefaultAiSettings(), enabled: true, model: "browser-fixture", apiKeyStored: true };
const idle: AssistantExecutionState = { status: "idle", promptLabel: null, result: null, error: null };
const context: AssistantBuildInput = {
  aiSettings: settings, locale: "zh-CN", activeJobsCount: 0, activeView: "system",
  bootstrap: fallbackBootstrap, storageReady: true, libraryPage: "catalog", overlayNames: [],
  runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null,
  selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
  selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
  selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null,
};
const noop = () => {};
let checks = 0;

function Harness({ ready, execution, messages, continuation, recoveryPending }: { recoveryPending: boolean; ready: boolean; execution: AssistantExecutionState; messages: AssistantChatMessage[]; continuation: AssistantContinuation | null }) {
  const [draft, setDraft] = useState("");
  const panelRef = useRef<HTMLDivElement | null>(null);
  return <AssistantPanel aiSettings={{ ...settings, enabled: ready }}
    assistantInput={context} draft={draft} execution={execution} panelRef={panelRef} messages={messages}
    conversations={[]} activeConversationId={null} onAction={noop}
    onClearAiSecret={async (next) => next} onClose={noop} onDeleteConversation={noop} onDraftChange={setDraft}
    onNewConversation={() => setDraft("")} onSelectConversation={noop} onRunPrompt={noop}
    onReady={noop} onSaveAiSettings={async (next) => next}
    continuation={continuation} recoveryPending={recoveryPending} onResume={() => { resumeRequests += 1; }}
    onStop={() => { stopRequests += 1; }}
    onSendMessage={(...args) => { submitted.push({ prompt: args[0], argumentCount: args.length }); }} />;
}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
const fixture = document.getElementById("fixture");
check(fixture, "Fixture root is missing");
const root = createRoot(fixture);
async function render(ready = true, execution = idle, messages: AssistantChatMessage[] = [], continuation: AssistantContinuation | null = null, recoveryPending = false) {
  await act(async () => {
    root.render(<StrictMode><I18nProvider><Harness ready={ready} execution={execution} messages={messages} continuation={continuation} recoveryPending={recoveryPending} /></I18nProvider></StrictMode>);
  });
  const deadline = performance.now() + 5000;
  while (!document.querySelector(".assistant-chat-input")) {
    check(performance.now() < deadline, `Composer did not mount: ${errors.join("; ")}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function input() {
  const element = document.querySelector<HTMLTextAreaElement>(".assistant-chat-input");
  check(element, "Composer is missing");
  return element;
}
function sendButton() {
  const element = document.querySelector<HTMLButtonElement>(".assistant-send-control button");
  check(element, "Send button is missing");
  return element;
}
async function type(value: string) {
  await act(async () => {
    const element = input();
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
    check(setter, "Native textarea setter is missing");
    setter.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function run() {
  await render();
  check(document.querySelector(".assistant-chat-composer select, .assistant-chat-composer input[type=checkbox]") === null,
    "Composer must not expose task goals, mod preservation or new/existing selectors");
  check(!document.body.textContent?.includes("任务目标"), "Manual goal label remained visible");
  checks++;
  const prompts = ["把人数改成 12", "这个服进不去，帮我修好", "帮我新建一个饥荒服", "你能干啥", "我们本机配置是什么", "没懂"];
  for (const prompt of prompts) {
    await type(prompt);
    check(!sendButton().disabled, "Natural-language request was blocked without a selected target");
    await act(async () => { sendButton().click(); });
    check(submitted[submitted.length - 1]?.prompt === prompt, "Request was replaced by a frontend task selection");
    check(input().value === "", "Sent request remained in the draft");
    check(submitted[submitted.length - 1]?.argumentCount === 1, "Composer attached unrelated UI diagnostics to the request");
    checks++;
  }
  const reply = "我可以查看服务器状态、分析日志，并按你的要求修改配置。需要执行修改时会先展示具体内容。";
  const chat: AssistantExecuteOperationOutput = {
    continuation: null, conversationId: "fixture-conversation", conversationRevision: 1,
    task: null, handled: false, action: "none", message: reply, requiresConfirmation: false,
    confirmationToken: null, confirmationExpiresAtUnixMs: null, planSummary: null,
    instanceId: null, moduleId: null, verification: null, followUp: null,
    appliedSettingsKeys: [], rejectedSettingsKeys: [], appliedPortNames: [], rejectedPortNames: [],
    workshopItemIds: [], modReferences: [], resolvedModIds: [], sourcePaths: [], runtimeCommands: [],
    runtimeResponseTexts: [], configDocumentCount: 0,
  };
  const outcome = await runAssistantWorkflow(chat, {
    confirmPreview: () => { throw new Error("Ordinary chat must not request confirmation"); },
    executeConfirmed: async () => { throw new Error("Ordinary chat must not execute an operation"); },
    onResult: async (operation) => {
      await render(true, idle, [{ id: "capability-reply", role: "assistant", ...assistantWorkflowResultMessage(operation) }]);
    },
  });
  check(outcome.completedSteps === 0, "Ordinary chat was counted as an executed operation");
  const visibleReply = document.querySelector("[role=log]")?.textContent ?? "";
  check(visibleReply.includes(reply), "Ordinary chat reply was hidden");
  check(!/任务|Task inconclusive|missing required fields/.test(visibleReply), "Ordinary chat exposed internal task interpretation state");
  check(document.querySelector(".assistant-operation-dialog") === null, "Ordinary chat opened an operation dialog");
  checks++;
  await type("稍后继续检查");
  await render(true, { ...idle, status: "running", promptLabel: "检查服务器" });
  check(!sendButton().disabled && input().value === "稍后继续检查", "Running request must preserve the draft and expose Stop");
  check(sendButton().getAttribute("aria-label") === "停止请求", "Stop control is not accessible");
  await act(async () => { sendButton().click(); });
  check(stopRequests === 1 && submitted.length === prompts.length, "Stop submitted a new request");
  await render(true, { ...idle, status: "running", stopping: true, promptLabel: "检查服务器" });
  check(sendButton().disabled && input().value === "稍后继续检查", "Stopping must wait for the backend and retain the draft");
  await act(async () => { sendButton().click(); });
  check(stopRequests === 1, "Stopping triggered duplicate cancellation");
  checks++;
  const checkpoint: AssistantContinuation = { reason: "model_slice", summary: "已读取配置，暂停等待继续分析。", canResume: true };
  await render(true, { ...idle, status: "paused" }, [], checkpoint);
  const paused = document.querySelector<HTMLElement>("[aria-label='任务已暂停']");
  check(paused?.textContent?.includes(checkpoint.summary), "Checkpoint summary is missing");
  const resume = paused.querySelector<HTMLButtonElement>("button");
  check(resume && !resume.disabled && resume.textContent === "继续", "Resumable checkpoint must expose Continue");
  await act(async () => { resume.click(); });
  check(resumeRequests === 1 && submitted.length === prompts.length, "Resume must not submit a new user prompt");
  checks++;
  await render(true, idle, [], { ...checkpoint, canResume: false });
  check(document.querySelector("[aria-label='任务已暂停'] button") === null, "Exhausted checkpoint must not offer resume");
  checks++;
  await render(true, idle, [], { ...checkpoint, reason: "investigation_failed" });
  check(document.querySelector("[aria-label='任务已暂停']")?.textContent?.includes("调查失败"), "Investigation retry explanation must be localized");
  checks++;
  await render(true, idle, [], null, true);
  const checkStatus = document.querySelector<HTMLButtonElement>("[aria-label='任务已暂停'] button");
  check(checkStatus?.textContent === "检查任务状态", "Unknown outcome must offer status inspection instead of blind resume");
  await act(async () => { checkStatus.click(); });
  check(resumeRequests === 2 && submitted.length === prompts.length, "Checking status must not submit another user message");
  check(document.querySelector("[aria-label='任务已暂停']")?.textContent?.includes("尚不确定"), "Unknown outcome must remain explicit");
  checks++;
  await render(true, { ...idle, status: "running" }, [], checkpoint);
  check(document.querySelector("[aria-label='任务已暂停']") === null, "Running request must not offer a duplicate resume");
  checks++;
  const progress = { text: "已读取日志：<script>不可执行的文本</script>", phase: "investigating",
    tools: [{ cursor: 1, name: "read_runtime", status: "completed" as const }], connectionIssue: false };
  await render(true, { ...idle, status: "running", progress });
  check(document.querySelector(".is-running .assistant-chat-message-body")?.textContent === progress.text, "Streamed text is missing or interpreted as HTML");
  check(document.querySelector(".assistant-progress-tools")?.textContent?.includes("读取运行日志和状态已完成"), "Actual tool status is not displayed in the user's language");
  await render(true, { ...idle, status: "running", progress: { ...progress, text: progress.text + "；正在校验。", connectionIssue: true } });
  check(document.querySelector(".is-running .assistant-chat-message-body")?.textContent?.endsWith("正在校验。"), "Incremental reply did not update");
  check(document.querySelector(".is-running [role=alert]")?.textContent?.includes("任务可能仍在运行"), "Lost progress transport must not claim the task stopped");
  checks++;
  await render(false);
  check(sendButton().disabled, "An unconfigured model must not accept execution");
  check(Boolean(sendButton().closest(".assistant-send-control")?.getAttribute("title")), "Disabled composer did not explain its state");
  checks++;
  const clarification = "你要修复的是哪一个服务器？请告诉我服务器名称。";
  await render(true, idle, [{ id: "clarification", role: "assistant", label: "LAN", content: clarification, state: "ready" }]);
  check(document.querySelector("[role=log]")?.textContent?.includes(clarification), "Backend clarification was hidden or replaced with a task picker");
  await type("是我的饥荒服务器");
  check(!sendButton().disabled, "Clarification reply was blocked without a manual selection");
  checks++;
  await render(true, { ...idle, status: "running", progress: { ...progress, text: "日志显示实例脚本存在空值访问。已核对配置，正在读取对应文件以确认修改范围。" } }, [
    { id: "live-user", role: "user", content: "这个服务器启动失败，检查原因并修好。", state: "ready" },
  ]);
  const composer = input().getBoundingClientRect();
  const button = sendButton().getBoundingClientRect();
  check(composer.width > 100 && composer.left >= 0 && composer.right <= innerWidth, "Composer is outside the viewport");
  check(button.width > 0 && button.right <= innerWidth && button.bottom <= innerHeight, "Send button is outside the viewport");
  checks++;
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  // Assertion phase is complete; the browser harness may now change visibility while capturing and closing the page.
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} completed checks`)), 15_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
