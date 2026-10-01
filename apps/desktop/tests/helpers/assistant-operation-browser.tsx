import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { AssistantOperationDialog } from "../../src/components/AssistantOperationDialog";
import { useAssistantOperationConfirmation } from "../../src/hooks/useAssistantOperationConfirmation";
import type { AssistantExecuteOperationOutput } from "../../src/types";
import type { AssistantConfirmationDecision } from "../../src/assistant-workflow";
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
let confirmation: ReturnType<typeof useAssistantOperationConfirmation> | null = null;
let checks = 0;
let sequence = 0;

function Harness({ scope }: { scope: string }) {
  confirmation = useAssistantOperationConfirmation(scope);
  return <>
    <button id="origin" type="button">实例修复预览</button>
    {confirmation.preview ? <AssistantOperationDialog key={confirmation.preview.confirmationToken}
      preview={confirmation.preview} onRespond={confirmation.respond} /> : null}
  </>;
}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
const fixture = document.getElementById("fixture");
check(fixture, "Fixture root is missing");
let root = createRoot(fixture);
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(pause);
  }
}
async function render(scope = "instance-one") {
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness scope={scope} /></I18nProvider></StrictMode>); });
  await settleUntil(() => Boolean(document.getElementById("origin")), "Harness did not mount");
}
function preview(overrides: Partial<AssistantExecuteOperationOutput> = {}): AssistantExecuteOperationOutput {
  return {
    continuation: null, conversationId: "fixture-conversation", conversationRevision: 1,
    handled: true, action: "patch_instance_text", message: "等待确认", requiresConfirmation: true,
    confirmationToken: `fixture-${++sequence}`, confirmationExpiresAtUnixMs: Date.now() + 60_000,
    planSummary: "修复实例内 Mod 的空值访问。仅替换下方片段，保留其余内容。",
    instanceId: "fixture-instance", moduleId: "dontstarve",
    appliedSettingsKeys: [], rejectedSettingsKeys: [], appliedPortNames: [], rejectedPortNames: [],
    workshopItemIds: [], modReferences: [], resolvedModIds: [], sourcePaths: [], runtimeCommands: [],
    runtimeResponseTexts: [], configDocumentCount: 0,
    fileChangePreview: { file: "mods/workshop-123/modmain.lua", sourceSha256: "a".repeat(64), resultSha256: "b".repeat(64),
      before: "local target = inst.components.combat.target\ntarget:DoTaskInTime(0, onHit)",
      after: "local target = inst.components.combat.target\nif target ~= nil then\n    target:DoTaskInTime(0, onHit)\nend" },
    ...overrides,
  };
}
function dialog() { return document.querySelector<HTMLDialogElement>(".assistant-operation-dialog"); }
function buttons() { return [...dialog()!.querySelectorAll<HTMLButtonElement>("footer button")]; }
async function open(value = preview()) {
  document.getElementById("origin")?.focus();
  let result!: Promise<AssistantConfirmationDecision>;
  await act(async () => { result = confirmation!.confirmPreview(value); });
  await settleUntil(() => dialog()?.open === true, "Dialog did not open");
  return { result, value };
}
async function key(name: "Tab" | "Escape" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
async function click(element: HTMLElement) { await act(async () => { element.click(); }); }

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await render();
  const unsafe = "  <img src=x onerror='window.unwanted=true'>\n\treturn false\n";
  let review = await open(preview({ fileChangePreview: { ...preview().fileChangePreview!, before: unsafe } }));
  check(dialog()!.querySelector("pre code")?.textContent === unsafe, "Review must preserve exact whitespace and text");
  check(dialog()!.querySelector("img") === null, "Review text must not become markup");
  check(document.activeElement === buttons()[0], "Initial focus must remain on Cancel after StrictMode replay");
  checks++;
  for (let index = 0; index < 6; index++) {
    await key("Tab");
    check(dialog()!.contains(document.activeElement), "Native Tab escaped the modal");
  }
  checks++;
  await key("Escape");
  check(await review.result === false, "Escape must cancel");
  check(document.activeElement === document.getElementById("origin"), "Closing must restore originating focus");
  checks++;

  review = await open();
  await click(buttons()[0]);
  check(await review.result === false, "Cancel button must cancel");
  checks++;

  review = await open();
  let resolutions = 0;
  void review.result.then(() => { resolutions++; });
  await act(async () => { confirmation!.respond(true); confirmation!.respond(true); });
  check(await review.result === true && resolutions === 1, "Repeated confirmation must resolve only once");
  checks++;

  review = await open();
  await act(async () => { dialog()!.close(); await review.result; });
  check(await review.result === false, "Native close must cancel");
  checks++;

  review = await open(preview({ confirmationExpiresAtUnixMs: Date.now() - 1 }));
  check(buttons()[1].disabled && Boolean(dialog()!.querySelector('[role="alert"]')), "Expired preview must disable confirmation visibly");
  await act(async () => { confirmation!.respond(true); });
  check(await review.result === false, "Expired preview must reject even a stale confirmation callback");
  checks++;

  review = await open(preview({ confirmationExpiresAtUnixMs: Date.now() + 100 }));
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 150)); });
  check(buttons()[1].disabled, "Open preview did not expire");
  await click(buttons()[0]);
  check(await review.result === false, "Expired preview cancel failed");
  checks++;

  review = await open();
  check(await confirmation!.confirmPreview(preview()) === false, "Concurrent confirmation must not replace the pending request");
  check(dialog()!.textContent?.includes(review.value.planSummary!), "Concurrent request changed preview");
  await click(buttons()[0]);
  check(await review.result === false, "Original request must remain cancellable");
  checks++;

  review = await open();
  await render("instance-two");
  check(await review.result === false && dialog() === null, "Changing instance scope must cancel pending review");
  checks++;

  review = await open();
  await act(async () => { root.unmount(); });
  check(await review.result === false, "Unmount must resolve pending confirmation");
  checks++;
  root = createRoot(fixture!);
  await render();

  review = await open(preview({ action: "customize_config", fileChangePreview: null, planSummary: "  保存配置\n保留原有 Mod  " }));
  check(dialog()!.querySelector(".assistant-operation-summary")?.textContent === review.value.planSummary, "Existing operation summary must remain exact");
  check(dialog()!.querySelector("pre") === null, "Existing operations must not invent file changes");
  buttons()[1].focus();
  await key("Enter");
  check(await review.result === true, "Native Enter must confirm the focused action");
  checks++;

  review = await open(preview({ fileChangePreview: null }));
  check(buttons()[1].disabled, "Missing patch preview must not be confirmable");
  await click(buttons()[0]);
  check(await review.result === false, "Missing patch preview must remain cancellable");
  checks++;

  const task = { id: "fixture-task", goal: "restore_service" as const, preserveExistingMods: true,
    operationLimit: 8, instanceId: "fixture-instance", moduleId: "dontstarve", status: "proposed" as const, requirements: [], checks: [] };
  review = await open(preview({ task }));
  const authorization = dialog()!.querySelector<HTMLInputElement>('input[type="checkbox"]');
  check(authorization && !authorization.checked, "Continuous repair must require an explicit unchecked choice");
  await click(authorization);
  await click(buttons()[1]);
  const decision = await review.result;
  check(typeof decision === "object" && decision.confirmed && decision.continueTask, "Explicit task authorization was not forwarded");
  checks++;

  review = await open(preview({ task, action: "install_server", fileChangePreview: null }));
  check(!dialog()!.querySelector('input[type="checkbox"]'), "Installation must not offer continuous repair authorization");
  await click(buttons()[0]);
  await review.result;
  review = await open(preview({ task }));
  check(!dialog()!.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked, "Authorization must not carry into the next preview");
  await click(buttons()[0]);
  await review.result;
  checks++;

  const batch = [{ file: "mods/server-rules/main.lua", sourceSha256: "a".repeat(64), resultSha256: "b".repeat(64),
    edits: [{ before: "target:ApplyDamage(amount)", after: "if target ~= nil then\n    target:ApplyDamage(amount)\nend" }] },
  { file: "scripts/server_config.json", sourceSha256: "c".repeat(64), resultSha256: "d".repeat(64),
    edits: [{ before: '"maxPlayers": 8', after: '"maxPlayers": 12' }] }];
  review = await open(preview({ task, action: "patch_instance_files", fileChangePreview: null, fileChangePreviews: batch,
    planSummary: "修复空值访问并调整人数上限。下面列出两个文件的准确修改片段。" }));
  check(dialog()!.querySelectorAll(".assistant-operation-file").length === 2, "Every proposed file must be named separately");
  check(dialog()!.querySelectorAll("pre code").length === 4, "Every file edit needs both before and after text");
  for (let index = 0; index < 7; index++) { await key("Tab"); check(dialog()!.contains(document.activeElement), "Batch preview lost keyboard containment"); }
  await click(buttons()[0]); await review.result;
  checks++;
  review = await open(preview({ task, action: "patch_instance_files", fileChangePreview: null, fileChangePreviews: [] }));
  check(buttons()[1].disabled, "Missing batch preview must not be confirmable");
  await click(buttons()[0]); await review.result;
  checks++;
  const lifecyclePreviews = [
    ["stop_server", "停止松果小队当前的服务器运行会话。"],
    ["restart_server", "停止当前会话，再启动并检查新会话；启动失败会保持停止。"],
    ["create_backup", "为已停止的松果小队创建存档备份，沿用当前备份保留策略。"],
    ["restore_backup", "从 backup-20260928 恢复松果小队存档，保留当前存档保护备份，恢复后不启动。"],
  ] as const;
  for (const [action, planSummary] of lifecyclePreviews) {
    review = await open(preview({ task: { ...task, goal: "apply_change" }, action,
      fileChangePreview: null, fileChangePreviews: [], planSummary }));
    check(dialog()!.querySelector(".assistant-operation-summary")?.textContent === planSummary,
      `${action} lost the exact reviewed target and action`);
    check(!dialog()!.querySelector('input[type="checkbox"]'), `${action} must not grant continuous repair`);
    check(!dialog()!.querySelector("pre") && !buttons()[1].disabled, `${action} must use its own confirmation without inventing a file diff`);
    await click(buttons()[0]);
    check(await review.result === false, `${action} must remain cancellable without applying it`);
    checks++;
  }
  await open(preview({ task: { ...task, goal: "apply_change" }, action: "restore_backup",
    fileChangePreview: null, fileChangePreviews: [], planSummary: lifecyclePreviews[3][1] }));
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} completed checks`)), 15_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
