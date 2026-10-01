import React, { type ComponentProps, type ReactNode } from "react";
import { AppShell } from "../../src/components/AppShell";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import type { BootstrapResponse } from "../../src/types";
import type { LocaleCode } from "../../src/i18n-config";

const noOperation = () => {};
const aiSettings = createDefaultAiSettings();

export function ServerArchivesShell({ bootstrap, locale, serverCount, children }: {
  bootstrap: BootstrapResponse; locale: LocaleCode; serverCount: number; children: ReactNode;
}) {
  const props: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView: "servers", theme: "dark", serverCount, aiSettings, activityText: "Archive fixture ready",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "Assistant", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale, activeJobsCount: 0, activeView: "servers", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false,
      runtimeRefreshIssue: null, selectedInstanceDetails: null, selectedInstanceId: null, selectedModuleId: null,
      selectedInstanceModuleDetails: null, selectedLaunchPlan: null, selectedLaunchPlanError: null,
      selectedLogDocument: null, selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: noOperation, onThemeChange: noOperation
  };
  return <AppShell {...props}>{children}</AppShell>;
}

type Check = (condition: unknown, description: string, diagnostics?: () => unknown) => asserts condition;

export function createArchiveLayoutAssertions(fixture: HTMLElement, check: Check, stableLayoutPhases: string[]) {
  function element(selector: string): HTMLElement {
    const node = fixture.querySelector<HTMLElement>(selector);
    if (!node) throw new Error(`Missing ${selector}`);
    return node;
  }
  function layoutSnapshot() {
    return [".server-detail-panel", ".workspace-metrics"].map((selector) => {
      const node = fixture.querySelector<HTMLElement>(selector);
      return { selector, node, content: node?.innerHTML, children: node ? [...node.children] : [], rect: node?.getBoundingClientRect() };
    });
  }
  function sameBounds(current: DOMRect, before: DOMRect, description: string) {
    check(["x", "y", "width", "height"].every((key) => Math.abs(current[key as keyof DOMRect] as number - (before[key as keyof DOMRect] as number)) <= 0.5),
      description, () => ({ before: before.toJSON(), after: current.toJSON() }));
  }
  function assertStableLayout(snapshot: ReturnType<typeof layoutSnapshot>, phase: string) {
    for (const before of snapshot) {
      const current = fixture.querySelector<HTMLElement>(before.selector);
      if (before.selector === ".workspace-metrics") {
        check(current === before.node && (!current || before.children.every((child, index) => current.children[index] === child)),
          `${phase}: metrics preserve their mounted DOM nodes`);
        check(current?.innerHTML === before.content, `${phase}: metrics preserve their normal-instance statistics`);
      }
      if (current && before.rect) sameBounds(current.getBoundingClientRect(), before.rect, `${phase}: ${before.selector} preserves its boundaries`);
    }
    check(!fixture.querySelector(".server-detail-panel .server-archive-details,.shell-content-scroll .shell-activity-notice"),
      `${phase}: archive metadata stays in tooltips and notifications remain in the activity bar`);
    stableLayoutPhases.push(phase);
  }
  function centeredIn(selector: string, container: string) {
    const target = element(selector); const parent = element(container);
    const rect = target.getBoundingClientRect(); const bounds = parent.getBoundingClientRect();
    const style = getComputedStyle(parent);
    const centerX = bounds.left + parent.clientLeft + (parent.clientWidth + parseFloat(style.paddingLeft) - parseFloat(style.paddingRight)) / 2;
    const centerY = bounds.top + parent.clientTop + (parent.clientHeight + parseFloat(style.paddingTop) - parseFloat(style.paddingBottom)) / 2;
    check(Math.abs(rect.x + rect.width / 2 - centerX) <= 2 && Math.abs(rect.y + rect.height / 2 - centerY) <= 2
      && getComputedStyle(target).textAlign === "center", `${selector} is horizontally and vertically centered inside ${container}`);
  }
  function workspaceGeometry() {
    return [".server-list-panel", ".server-list-panel > .table-list", ".server-detail-panel", ".workspace-metrics", ".shell-activity-bar"]
      .map((selector) => ({ selector, rect: element(selector).getBoundingClientRect() }));
  }
  function unchangedWorkspace(snapshot: ReturnType<typeof workspaceGeometry>, phase: string) {
    for (const before of snapshot) sameBounds(element(before.selector).getBoundingClientRect(), before.rect, `${phase}: ${before.selector} does not move`);
  }
  function notice(text: string, tone: "error" | "warning" | "success") {
    const selected = [...fixture.querySelectorAll<HTMLElement>(`.shell-activity-notice.is-${tone}`)]
      .find((node) => node.querySelector(".shell-activity-notice-text")?.textContent?.includes(text));
    check(selected && element(".shell-activity-bar").contains(selected), `${tone} notification appears in the real activity bar: ${text}`, () => ({
      locale: document.documentElement.lang,
      focusedElement: document.activeElement?.className,
      barFocused: element(".shell-activity-bar").matches(":focus-within"),
      barHovered: element(".shell-activity-bar").matches(":hover"),
      barContents: element(".shell-activity-bar").textContent
    }));
    check(!element(".shell-content-scroll").textContent?.includes(text), `Operation feedback does not occupy the workspace: ${text}`);
    const message = selected.querySelector<HTMLElement>(".shell-activity-notice-text")!;
    check(message.title === message.textContent, `Full ${tone} notification remains available in its title`);
    check(selected.getAttribute("role") === (tone === "error" ? "alert" : "status"), `${tone} notification retains its accessible role`);
    return selected;
  }
  return { layoutSnapshot, assertStableLayout, centeredIn, workspaceGeometry, unchangedWorkspace, notice };
}
