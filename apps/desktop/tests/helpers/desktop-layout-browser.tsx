import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { I18nProvider } from "../../src/i18n";
import { AppShell } from "../../src/components/AppShell";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const noOperation = () => {};
let checks = 0;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = fixture.querySelector<T>(selector);
  check(selected, `Missing ${selector}`);
  return selected;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function frame() {
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
}
async function pressEnter(button: HTMLElement) {
  button.scrollIntoView({ block: "nearest", inline: "nearest" });
  assertVisible(button, "Keyboard target");
  await act(async () => {
    button.focus();
    check(document.activeElement === button, `Control must receive keyboard focus: ${button.outerHTML}`);
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter dispatch failed");
  });
}
async function input(selector: string, value: string) {
  await act(async () => {
    const target = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function assertVisible(target: HTMLElement, label: string) {
  const box = target.getBoundingClientRect();
  check(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= innerWidth + 1
    && box.top >= -1 && box.bottom <= innerHeight + 1, `${label} is outside the viewport: ${JSON.stringify(box.toJSON())}`);
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  check(hit && (target.contains(hit) || hit.contains(target)), `${label} is covered or clipped`);
}
function assertCoverConfirmation(card: HTMLElement, label: string, visible = true) {
  const review = card.querySelector<HTMLElement>(".server-list-card-delete.is-confirming > .inline-confirm-review");
  check(review && !document.querySelector(".inline-confirm-popover"), `${label} must replace the cover inside its instance card`);
  const cover = card.querySelector<HTMLElement>(".server-list-card-media")!.getBoundingClientRect();
  const footer = card.querySelector<HTMLElement>(".server-list-card-footer")!.getBoundingClientRect();
  check(getComputedStyle(card.querySelector(".module-cover")!).visibility === "visible"
    && getComputedStyle(card.querySelector(".server-list-card-status")!).visibility === "hidden", `${label} must keep the cover behind the translucent review and hide the status`);
  for (const target of [review, ...review.querySelectorAll<HTMLElement>(".inline-confirm-message, button")]) {
    const box = target.getBoundingClientRect();
    check(box.width > 0 && box.height > 0 && box.left >= cover.left - 1 && box.right <= cover.right + 1
      && box.top >= cover.top - 1 && box.bottom <= cover.bottom + 1 && box.bottom <= footer.top + 1
      && target.scrollWidth <= target.clientWidth + 1
      && (target.classList.contains("inline-confirm-message")
        ? getComputedStyle(target).overflowY === "auto" : target.scrollHeight <= target.clientHeight + 1),
      `${label} text and controls must fit the cover without covering the footer: ${JSON.stringify(box.toJSON())}`);
    if (visible) assertVisible(target, label);
  }
  return review;
}
async function readFullRemovalPlan(review: HTMLElement) {
  const message = review.querySelector<HTMLElement>(".inline-confirm-message")!;
  const buttons = [...review.querySelectorAll<HTMLButtonElement>("button")];
  const before = buttons.map((button) => button.getBoundingClientRect().toJSON());
  check(message.tabIndex === 0 && message.scrollHeight > message.clientHeight,
    "The complete removal plan must be a keyboard-accessible local scroll region");
  check(message.textContent?.includes("fixture/instance/saves") && message.textContent.includes("fixture/instance/backups")
    && message.textContent.includes("fixture/library") && message.textContent.includes("fixture/external-world"),
    "Removal confirmation must retain every deleted and preserved data path");
  await act(async () => {
    message.focus();
    check(document.activeElement === message, "The removal plan must receive keyboard focus");
    const response = await fetch(`/__reliability_key/${nonce}/End`, { method: "POST" });
    check(response.ok, "Native End dispatch failed");
  });
  await settleUntil(() => message.scrollTop > 0 && message.scrollHeight - message.clientHeight - message.scrollTop <= 1,
    "Native End must reveal the end of the complete removal plan");
  const lastText = message.lastChild!;
  const lastCharacter = document.createRange();
  lastCharacter.setStart(lastText, (lastText.textContent?.length ?? 1) - 1);
  lastCharacter.setEnd(lastText, lastText.textContent?.length ?? 1);
  const lastBounds = lastCharacter.getBoundingClientRect();
  const bounds = message.getBoundingClientRect();
  check(lastBounds.height > 0 && lastBounds.top >= bounds.top - 1 && lastBounds.bottom <= bounds.bottom + 1,
    "The final preserved path must be visibly readable inside the scrolled plan");
  check(JSON.stringify(buttons.map((button) => button.getBoundingClientRect().toJSON())) === JSON.stringify(before),
    "Reading the full removal plan must not move the confirmation buttons");
}
function assertCoverRestored(card: HTMLElement, label: string) {
  check(!card.querySelector(".inline-confirm-review") && getComputedStyle(card.querySelector(".module-cover")!).visibility === "visible"
    && getComputedStyle(card.querySelector(".server-list-card-status")!).visibility === "visible", `${label} must restore the cover and status`);
}
function reveal(target: HTMLElement, label: string) {
  target.scrollIntoView({ block: "nearest", inline: "nearest" });
  assertVisible(target, label);
}
function horizontalFit(selector: string) {
  const target = element(selector);
  const outer = target.getBoundingClientRect();
  const overflow = [...target.querySelectorAll<HTMLElement>("*")].filter((child) => {
    const box = child.getBoundingClientRect();
    return box.width > 0 && (box.left < outer.left - 1 || box.right > outer.right + 1
      || child.scrollWidth > child.clientWidth + 1);
  }).slice(0, 6).map((child) => {
    const { x, y, width, height } = child.getBoundingClientRect();
    return { tag: child.tagName, class: child.className, scrollWidth: child.scrollWidth, clientWidth: child.clientWidth,
      rect: { x, y, width, height } };
  });
  check(target.clientWidth > 0 && target.scrollWidth <= target.clientWidth + 1,
    `${selector} overflows horizontally: ${target.scrollWidth}/${target.clientWidth}; descendants: ${JSON.stringify(overflow)}`);
}
function shellGeometry() {
  check(document.documentElement.scrollWidth <= innerWidth + 1, "The window has horizontal overflow");
  check(document.documentElement.scrollHeight <= innerHeight + 1, "The whole window scrolls instead of its content");
  for (const selector of [".shell-header", ".shell-header-tools", ".shell-topnav", ".shell-content-scroll", ".shell-activity-bar"])
    horizontalFit(selector);
  for (const selector of [".shell-theme-icon-button", ".shell-locale-button", ".shell-topnav-item", ".shell-activity-bar"])
    assertVisible(element(selector), selector);
  for (const control of fixture.querySelectorAll<HTMLElement>(".shell-window-button")) assertVisible(control, "Window control");
}
function typography(selector: string) {
  const style = getComputedStyle(element(selector));
  return { size: style.fontSize, family: style.fontFamily, lineHeight: style.lineHeight, weight: style.fontWeight };
}

async function run() {
  for (const font of ['400 13px "Inter"', '500 13px "Inter"', '600 16px "Inter"', 'italic 400 13px "Inter"']) {
    const loaded = await document.fonts.load(font, "LanGame 0123456789");
    check(loaded.length > 0 && loaded.every((face) => face.status === "loaded"), `Bundled UI font did not load: ${font}`);
  }
  await document.fonts.ready;
  // Native/storage/network boundaries use the established development mock.
  // AppShell, ServersView, settings navigation and all application CSS are real.
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(original, "Development mock must provide a Minecraft instance");
  const stored = await readInstanceDetails(original.id);
  const moduleDetails = await readModuleDetails("minecraft");
  const instance = { ...original, module_id: "fixture-layout", name: "布局验证世界 01", status: "Stopped" as const, active_process_count: 0 };
  const instances = Array.from({ length: 30 }, (_, index) => ({ ...instance,
    id: index === 0 ? instance.id : `fixture-layout-${index}`, name: `布局验证世界 ${String(index + 1).padStart(2, "0")}` }));
  const details = { ...stored, summary: instance, active_run: null };
  const lines = Array.from({ length: 100 }, (_, index) => `[${String(index).padStart(3, "0")}] Layout fixture console output`);
  const runtime = { ...await readInstanceRuntime(original.id), recent_runs: [], diagnostics: [],
    log_tail: { source_path: "fixture/runtime.log", lines, total_lines: lines.length, truncated: false, read_error: null } };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (command === "list_instance_archives") return { archives: [], pending_deletions: [], issues: [] };
    if (command === "inspect_instance_removal") {
      const input = args.input as { instance_id?: string } | undefined;
      check(input?.instance_id === instance.id, "Removal inspection must target the reviewed fixture instance");
      return { program_path: "fixture/library", data_path: "fixture/instance", remove_program: false,
        preserved_program_path: "fixture/library", owned_data_paths: ["fixture/instance/saves", "fixture/instance/backups"],
        preserved_external_saves_path: "fixture/external-world" };
    }
    if (command === "read_instance_connection_info_from_storage") {
      const ids = args.instanceIds as string[];
      check(Array.isArray(ids) && ids.every((id) => instances.some((entry) => entry.id === id)),
        "Connection reads must remain inside this fixture");
      return ids.map((id) => ({ instance_id: id, bind_ip: instance.bind_ip,
        ports: stored.ports, settings_json: stored.settings_json }));
    }
    throw new Error(`Unexpected layout command: ${command}`);
  } } });
  const aiSettings = createDefaultAiSettings();
  let selectedCount = 0;
  let createCount = 0;
  const deleteRequests: string[] = [];
  let themeCount = 0;
  let navigationCount = 0;
  const viewProps: ComponentProps<typeof ServersView> = {
    aiSettings, assistantCanRun: false, bindAddressCandidates: [], instances,
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: null,
    runtime, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: noOperation, onResumeAutoRefresh: noOperation, onSelectInstance: () => { selectedCount++; },
    onStart: noOperation, onStop: noOperation, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation,
    onCreateInstance: () => { createCount++; }, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: async (id) => { deleteRequests.push(id); }, onRestoreBackup: noOperation,
    onRenameBackup: async () => true, onDeleteBackup: async () => {}, onSaveSettings: async () => undefined,
    onSaveAutostart: async () => {}, onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView: "servers", theme: "dark", serverCount: instances.length, aiSettings, activityText: "界面已就绪",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "助手", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale: "zh-CN", activeJobsCount: 0, activeView: "servers", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
      selectedInstanceDetails: details, selectedInstanceId: instance.id, selectedModuleId: null, selectedInstanceModuleDetails: null,
      selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: runtime.log_tail,
      selectedModuleDetails: null, selectedRuntime: runtime, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: () => { navigationCount++; }, onThemeChange: (theme) => {
      themeCount++; shellProps.theme = theme; document.documentElement.dataset.theme = theme; render();
    }
  };
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
      <AppShell {...shellProps}><ServersView {...viewProps} /></AppShell>
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector("pre.server-runtime-console")), "Server console did not mount");
  await frame();
  shellGeometry();
  for (const selector of [".servers-page", ".server-layout-grid", ".server-list-panel", ".server-detail-panel", ".server-runtime-console-frame"])
    horizontalFit(selector);
  reveal(element(".server-detail-subheader"), "Server detail tabs");
  reveal(element(".server-runtime-console-command-form"), "Console command row");
  checks++;

  const firstCard = element(".server-list-card");
  const deleteTrigger = () => firstCard.querySelector<HTMLButtonElement>(".server-list-card-delete > button")!;
  const cardBox = firstCard.getBoundingClientRect();
  const secondCardTop = firstCard.nextElementSibling!.getBoundingClientRect().top;
  const footerParts = [...firstCard.querySelectorAll<HTMLElement>(".server-list-card-footer, .server-list-card-copy, .server-list-card-network, .server-list-card-primary-action")]
    .map((target) => ({ target, box: target.getBoundingClientRect() }));
  await pressEnter(deleteTrigger());
  const deleteReview = assertCoverConfirmation(firstCard, "Delete confirmation");
  const afterDeleteBox = firstCard.getBoundingClientRect();
  check(afterDeleteBox.height === cardBox.height && afterDeleteBox.width === cardBox.width
    && firstCard.nextElementSibling!.getBoundingClientRect().top === secondCardTop,
    "Opening delete confirmation must not stretch the card or move neighboring instances");
  check(footerParts.every(({ target, box }) => {
    const current = target.getBoundingClientRect();
    return current.x === box.x && current.y === box.y && current.width === box.width && current.height === box.height;
  }), "Deletion confirmation must preserve the footer, name, connection and primary-action positions");
  check(deleteReview.textContent?.includes("永久删除") && deleteReview.textContent.includes("存档和备份")
    && deleteReview.textContent.includes("外部存档"), "Compact deletion confirmation explains permanent owned-data deletion and preserved external saves");
  check(deleteRequests.length === 0, "Opening deletion confirmation must not delete an instance");
  const cancelDelete = deleteReview.querySelector<HTMLButtonElement>("button")!;
  check(document.activeElement === cancelDelete, "Deletion review must initially focus Cancel");
  await readFullRemovalPlan(deleteReview);
  const deletionList = element(".server-list-panel > .table-list");
  const beforeScroll = deletionList.scrollTop;
  const beforeScrollCardTop = firstCard.getBoundingClientRect().top;
  const beforeScrollReviewTop = deleteReview.getBoundingClientRect().top;
  await act(async () => { deletionList.scrollTop = beforeScroll + 24; });
  await settleUntil(() => {
    const cardDelta = firstCard.getBoundingClientRect().top - beforeScrollCardTop;
    const reviewDelta = deleteReview.getBoundingClientRect().top - beforeScrollReviewTop;
    return cardDelta < -1 && Math.abs(reviewDelta - cardDelta) <= 1;
  }, "Cover confirmation must move with its card while the instance list scrolls");
  assertCoverConfirmation(firstCard, "Scrolled delete confirmation", false);
  await act(async () => { deletionList.scrollTop = beforeScroll; });
  await settleUntil(() => Math.abs(deleteReview.getBoundingClientRect().top - beforeScrollReviewTop) <= 1,
    "Cover confirmation must return with its card after scrolling back");
  assertCoverConfirmation(firstCard, "Restored delete confirmation");
  checks++;
  await pressEnter(cancelDelete);
  assertCoverRestored(firstCard, "Cancelling deletion");
  check(document.activeElement === deleteTrigger() && deleteRequests.length === 0,
    "Cancelling deletion must preserve the instance and restore the recreated trigger's focus");
  await pressEnter(deleteTrigger());
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
    check(response.ok, "Native Escape dispatch failed");
  });
  assertCoverRestored(firstCard, "Escaping deletion");
  check(document.activeElement === deleteTrigger() && deleteRequests.length === 0, "Escape must preserve the instance and restore trigger focus");
  await pressEnter(deleteTrigger());
  const outsideDelete = element<HTMLInputElement>(".server-list-search");
  await act(async () => {
    outsideDelete.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); outsideDelete.click(); outsideDelete.focus();
  });
  assertCoverRestored(firstCard, "Outside click");
  check(document.activeElement === outsideDelete && deleteRequests.length === 0, "Outside click must dismiss deletion without stealing focus or executing");
  checks++;
  await pressEnter(deleteTrigger());
  const confirmDelete = assertCoverConfirmation(firstCard, "Reopened delete confirmation").querySelector<HTMLButtonElement>(".inline-confirm-submit")!;
  await act(async () => { confirmDelete.click(); });
  check(deleteRequests.length === 1 && deleteRequests[0] === instance.id, "Confirmed deletion must target the reviewed instance exactly once");
  await settleUntil(() => !firstCard.querySelector(".inline-confirm-review"), "Confirmed deletion must complete its archive inventory refresh");
  assertCoverRestored(firstCard, "Completed deletion");
  checks++;

  const measured = {
    navigation: typography(".shell-topnav-label"), input: typography(".server-list-search"),
    metadata: typography(".server-list-result-count"), console: typography("pre.server-runtime-console")
  };
  check(measured.navigation.size === "13px" && measured.input.size === "13px", `Navigation and inputs must use 13px: ${JSON.stringify(measured)}`);
  check(measured.navigation.weight === "500" && measured.input.weight === "400", "Navigation should be medium weight and input text regular");
  check(measured.console.size === "13px", "Console output must use the 13px code role");
  check(parseFloat(measured.metadata.size) >= 12, "Metadata must remain at least 12px");
  check(measured.console.family !== measured.navigation.family, "Console must use the dedicated monospace family");
  checks++;

  const list = element(".server-list-panel > .table-list");
  check(list.scrollHeight > list.clientHeight + 10, "Long server list must own a scroll area");
  const last = element<HTMLElement>(".server-list-panel .server-list-card:last-child .server-list-card-hitarea");
  await pressEnter(last);
  check(list.scrollTop > 0 && selectedCount === 1, "Scrolling the server list must expose an operable final instance");
  list.scrollTop = 0;
  checks++;
  const terminal = element("pre.server-runtime-console");
  check(terminal.scrollHeight > terminal.clientHeight + 10, "Long terminal output must own a scroll area");
  terminal.scrollTop = 0;
  check(terminal.textContent?.includes("[000]"), "First terminal line was lost");
  terminal.scrollTop = terminal.scrollHeight;
  check(terminal.scrollTop > 0 && terminal.textContent?.includes("[099]"), "Terminal cannot reach its latest output");
  shellGeometry();
  checks++;

  await input(".server-list-search", "布局验证世界 30");
  check(fixture.querySelectorAll(".server-list-card").length === 1, "Server search is not operable");
  await input(".server-list-search", "");
  check(fixture.querySelectorAll(".server-list-card").length === 30, "Clearing search lost server rows");
  checks++;
  const filter = element(".server-list-filter");
  const runningFilter = element<HTMLInputElement>(".server-list-filter input");
  check(runningFilter.type === "checkbox" && runningFilter.getAttribute("aria-label") === "仅显示运行中的实例",
    "Running filter must remain an accessibly named native checkbox");
  check(!filter.hasAttribute("title") && !runningFilter.hasAttribute("title"), "Running filter must not duplicate its bubble with a native title");
  check(!filter.querySelector("span:not(.configuration-field-help-a11y)")
    && [...filter.childNodes].every((node) => node.nodeType !== Node.TEXT_NODE || !node.textContent?.trim()),
    "Running filter must display only its checkbox");
  async function filterTooltip() {
    await settleUntil(() => {
      const tooltip = document.querySelector(".configuration-field-help-tooltip.is-visible");
      return Boolean(tooltip && Number(getComputedStyle(tooltip).opacity) === 1);
    }, "Running filter help bubble did not appear");
    const tooltip = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")!;
    check(tooltip.textContent === "仅显示运行中的实例", "Running filter bubble must explain the checkbox");
    const descriptionId = element<HTMLInputElement>(".server-list-filter input").getAttribute("aria-describedby");
    const description = descriptionId && document.getElementById(descriptionId);
    check(description && description.getAttribute("role") === "tooltip" && description.textContent === tooltip.textContent,
      "Running filter checkbox must reference its accessible help text");
    const box = tooltip.getBoundingClientRect();
    check(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= innerWidth + 1
      && box.top >= -1 && box.bottom <= innerHeight + 1 && tooltip.scrollWidth <= tooltip.clientWidth + 1,
      "Running filter bubble must fit the viewport without clipped text");
  }
  function emptyFilteredList() {
    check(list.childElementCount === 0 && !list.textContent?.trim(), "Unmatched filters must leave a blank list without empty-state text or reset actions");
  }
  await act(async () => { filter.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" })); });
  await filterTooltip();
  await act(async () => { filter.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "mouse", relatedTarget: document.body })); });
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Running filter hover bubble did not dismiss");
  await act(async () => { runningFilter.focus(); });
  check(document.activeElement === runningFilter, "Running filter must receive keyboard focus");
  await filterTooltip();
  await act(async () => { runningFilter.click(); });
  check(runningFilter.checked, "Running filter must toggle on");
  emptyFilteredList();
  await act(async () => { runningFilter.click(); });
  check(!runningFilter.checked && fixture.querySelectorAll(".server-list-card").length === 30,
    "Disabling running filter must restore all stopped instances");
  checks++;
  await input(".server-list-search", "没有匹配的实例");
  emptyFilteredList();
  await input(".server-list-search", "");
  check(fixture.querySelectorAll(".server-list-card").length === 30, "Clearing unmatched search must restore all instances");
  await input(".server-list-search", "布局验证世界 30");
  await act(async () => { runningFilter.click(); });
  emptyFilteredList();
  check(element<HTMLInputElement>(".server-list-search").value === "布局验证世界 30", "Running filter must preserve the search query");
  await act(async () => {
    viewProps.instances = instances.map((item, index) => index === 29 ? { ...item, status: "Running", active_process_count: 1 } : item);
    render();
  });
  check(fixture.querySelectorAll(".server-list-card").length === 1, "Combined filters must show the matching running instance");
  await input(".server-list-search", "布局验证世界 01");
  emptyFilteredList();
  await act(async () => { runningFilter.click(); });
  check(fixture.querySelectorAll(".server-list-card").length === 1
    && element<HTMLInputElement>(".server-list-search").value === "布局验证世界 01",
    "Disabling running filter must retain the independently matching search");
  await input(".server-list-search", "");
  await act(async () => { viewProps.instances = instances; render(); });
  check(fixture.querySelectorAll(".server-list-card").length === 30, "Resetting filters must restore the complete instance list");
  checks++;
  await pressEnter(element(".shell-theme-icon-button"));
  check(themeCount === 1 && shellProps.theme === "light", "Theme action did not reach its callback");
  await pressEnter(element(".server-list-card-delete > button"));
  const lightReview = assertCoverConfirmation(element(".server-list-card"), "Light-theme deletion confirmation");
  await pressEnter(lightReview.querySelector<HTMLButtonElement>("button")!);
  assertCoverRestored(element(".server-list-card"), "Light-theme cancellation");
  shellGeometry();
  await pressEnter(element(".shell-theme-icon-button"));
  check(themeCount === 2 && shellProps.theme === "dark", "Theme action could not restore dark mode");
  checks++;
  await pressEnter(element(".shell-topnav-item"));
  check(navigationCount === 1, "Primary navigation is not keyboard operable");
  checks++;
  await pressEnter(element(".shell-locale-button"));
  await settleUntil(() => element(".shell-locale-button").textContent === "ZH", "English interface did not load");
  await pressEnter(element(".server-list-card-delete > button"));
  const englishReview = assertCoverConfirmation(element(".server-list-card"), "English deletion confirmation");
  await pressEnter(englishReview.querySelector<HTMLButtonElement>("button")!);
  assertCoverRestored(element(".server-list-card"), "English cancellation");
  shellGeometry();
  horizontalFit(".server-list-panel");
  horizontalFit(".server-detail-subheader");
  await pressEnter(element(".shell-locale-button"));
  await settleUntil(() => element(".shell-locale-button").textContent === "EN", "Chinese interface did not restore");
  checks++;

  viewProps.section = "settings";
  viewProps.selectedDetails = { ...details, summary: { ...instance, module_id: "minecraft" } };
  viewProps.selectedModuleDetails = moduleDetails;
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector(".configuration-workspace")), "Configuration workspace did not mount");
  shellGeometry();
  for (const selector of [".configuration-workspace", ".configuration-workspace__body", ".configuration-workspace__main"])
    horizontalFit(selector);
  reveal(element(".shell-activity-notice"), "Configuration save status");
  checks++;
  const toggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  const narrow = innerWidth <= 1100;
  if (narrow) {
    reveal(toggle, "Configuration navigation toggle");
    check(toggle.getAttribute("aria-expanded") === "false", "Narrow configuration navigation must start collapsed");
    check(element(".configuration-workspace__sidebar").getBoundingClientRect().height === 0, "Collapsed navigation still consumes configuration space");
    await pressEnter(toggle);
    check(toggle.getAttribute("aria-expanded") === "true", "Narrow configuration navigation cannot expand");
    assertVisible(element(".configuration-search input"), "Configuration search");
    await act(async () => {
      const search = element<HTMLInputElement>(".configuration-search input");
      search.focus();
      search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true }));
    });
    check(toggle.getAttribute("aria-expanded") === "true", "Cancelling IME composition must not close configuration navigation");
    await act(async () => {
      const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
      check(response.ok, "Native Escape dispatch failed");
    });
    check(toggle.getAttribute("aria-expanded") === "false" && document.activeElement === toggle,
      "Escape must close navigation and return focus to its toggle");
    await pressEnter(toggle);
  }
  const alternatives = [...fixture.querySelectorAll<HTMLButtonElement>(".configuration-section-navigation__button:not(.is-active)")];
  const target = alternatives.find((button) => button.getBoundingClientRect().height > 0);
  check(target, "Configuration fixture must expose another category");
  await pressEnter(target);
  check(target.getAttribute("aria-current") === "page", "Selecting a category did not update configuration");
  if (narrow) {
    check(toggle.getAttribute("aria-expanded") === "false", "Selecting a category must collapse narrow navigation");
    check(document.activeElement === toggle, "Category selection must preserve keyboard focus on its visible toggle");
  }
  checks++;

  // Exercise the real field-search navigation, including focus after collapse.
  if (narrow) await pressEnter(toggle);
  await input(".configuration-search input", "motd");
  await settleUntil(() => Boolean(fixture.querySelector(".configuration-search-results button")), "Native key search did not produce a configuration result");
  await pressEnter(element(".configuration-search-results button"));
  check(element(".configuration-workspace__main").contains(document.activeElement), "Searching a field did not move focus to its editor");
  if (narrow) check(toggle.getAttribute("aria-expanded") === "false", "Field selection must collapse narrow navigation");
  assertVisible(document.activeElement as HTMLElement, "Selected configuration editor");
  check(getComputedStyle(document.activeElement as HTMLElement).fontWeight === "400", "Configuration editor contents must remain regular weight");
  horizontalFit(".configuration-workspace__main");
  checks++;

  const configurationMeta = typography(".shell-activity-notice");
  check(parseFloat(configurationMeta.size) >= 12, "Save status metadata must remain readable");
  // Help is an intentional interactive overlay; dismiss it before checking the shell beneath it.
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
    check(response.ok, "Native Escape dispatch failed");
  });
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Field help did not dismiss with Escape");
  shellGeometry();
  checks++;
  const configured = viewProps.selectedDetails;
  viewProps.instances = [];
  viewProps.selectedInstanceId = null;
  viewProps.selectedDetails = null;
  await act(async () => { render(); });
  reveal(element(".server-workspace-empty h3"), "Empty state title");
  const title = typography(".server-workspace-empty h3");
  check(title.size === "16px" && title.weight === "600", "Section headings must use the compact 16px semibold role");
  const createButton = element(".server-workspace-empty .primary-button");
  const createHeight = createButton.getBoundingClientRect().height;
  check(createHeight >= 28 && createHeight <= 30.5, `Create action must retain a compact, usable target: ${createHeight}`);
  check(createButton.scrollWidth <= createButton.clientWidth + 1 && createButton.scrollHeight <= createButton.clientHeight + 1,
    "Compact create action must not clip its label");
  await pressEnter(element(".server-workspace-empty .primary-button"));
  check(createCount === 1, "Empty state create action is not operable");
  shellGeometry();
  checks++;
  // Screenshots end on a populated settings page, after all interaction checks.
  viewProps.instances = instances;
  viewProps.selectedInstanceId = instance.id;
  viewProps.selectedDetails = configured;
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector(".configuration-workspace")), "Configuration workspace did not remount");
  element(".shell-content-scroll").scrollTop = 0;
  await act(async () => { element<HTMLInputElement>(".server-list-filter input").focus(); });
  await filterTooltip();
  await pressEnter(element(".server-list-card-actions > .inline-confirm-action > button"));
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Filter bubble must dismiss before deletion review");
  const finalReview = assertCoverConfirmation(element(".server-list-card"), "Final deletion confirmation");
  await readFullRemovalPlan(finalReview);
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, device_scale_factor: devicePixelRatio,
    viewport: { width: innerWidth, height: innerHeight }, typography: { ...measured, configurationMeta, title } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Layout interaction stalled after ${checks} checks`)), 25000);
})]).finally(() => {
  clearTimeout(watchdog);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
}).catch((error) => ({ status: "failed", checks,
  error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
