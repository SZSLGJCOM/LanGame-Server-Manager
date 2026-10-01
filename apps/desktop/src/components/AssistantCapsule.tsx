import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import type {
  AssistantActionId,
  AssistantBuildInput,
  AssistantCapsuleModel,
  AssistantPromptCard
} from "../assistant-types";
import type { AssistantConversationSummary } from "../assistant-conversations";
import type { AiSettings, PersistAiSettings } from "../ai-settings";
import { useI18n } from "../i18n";
import type { AssistantChatMessage, AssistantExecutionState } from "../types";
import { LanMark } from "./LanMark";
import { containAssistantFocus, focusAssistantPanel } from "./assistant-focus";
import { assistantPanelStyle } from "./assistant-panel-position";
import { ShellIcon } from "./ShellIcon";
import { createDeferredModule } from "../deferred-module";
import { useDeferredModule } from "../hooks/useDeferredModule";
import { AssistantIslandSurface } from "./AssistantIslandSurface";

const assistantPanelModule = createDeferredModule(
  () => import("./AssistantPanel").then((module) => module.AssistantPanel),
  () => import("./AssistantPanel?module-retry").then((module) => module.AssistantPanel)
);

function preloadAssistantPanel() {
  // Speculative failures are shown only if the operator opens LAN; opening can retry.
  void assistantPanelModule.load().catch(() => undefined);
}

interface AssistantCapsuleProps {
  aiSettings: AiSettings;
  assistant: AssistantCapsuleModel;
  assistantDraft: string;
  assistantInput: AssistantBuildInput;
  execution: AssistantExecutionState;
  confirmationOpen?: boolean;
  messages: AssistantChatMessage[];
  conversations: AssistantConversationSummary[];
  activeConversationId: string | null;
  onAction: (actionId: AssistantActionId) => void | Promise<void>;
  onClearAiSecret: PersistAiSettings;
  onDeleteConversation: (conversationId: string) => void;
  onDraftChange: (value: string) => void;
  onNewConversation: () => void;
  onSelectConversation: (conversationId: string) => void;
  onRunPrompt: (prompt: AssistantPromptCard) => void | Promise<void>;
  onSaveAiSettings: PersistAiSettings;
  recoveryPending?: boolean;
  continuation?: import("../types").AssistantContinuation | null;
  onResume?: () => void | Promise<void>;
  onStop?: () => void | Promise<void>;
  onSendMessage: (message: string) => void | Promise<void>;
}

function AssistantPanelFallback({
  panelTitle,
  panelRef,
  anchorRect,
  failed,
  canRetry,
  onReady,
  onClose,
  onRetry
}: {
  panelTitle: string;
  panelRef: RefObject<HTMLDivElement | null>;
  anchorRect: DOMRect | null;
  failed: boolean;
  canRetry: boolean;
  onReady: () => void;
  onClose: () => void;
  onRetry: () => void;
}) {
  const { t } = useI18n();
  const style = assistantPanelStyle(anchorRect);

  useLayoutEffect(() => {
    onReady();
  }, [onReady, failed]);

  return (
    <div
      ref={panelRef}
      className={`assistant-panel assistant-panel--chat${style ? " is-anchored" : ""}`}
      style={style}
      role="group"
      aria-label={panelTitle}
      tabIndex={-1}
      data-no-window-drag="true"
    >
      <div className="assistant-panel-head">
        <div className="assistant-panel-identity">
          <div className="assistant-panel-avatar" aria-hidden="true"><LanMark className="assistant-avatar-mark" /></div>
        </div>
        <div className="assistant-panel-controls">
          <button type="button" className="assistant-close-button" aria-label={t("assistant.closeLabel", undefined, "Close LAN")} onClick={onClose}>
            <ShellIcon name="x" className="assistant-close-icon" width={18} height={18} />
          </button>
        </div>
      </div>
      <div className="assistant-load-state" role={failed ? "alert" : "status"} aria-live="polite" aria-atomic="true">
        <strong>{failed
          ? t("assistant.loadFailedTitle", undefined, "LAN could not open")
          : t("assistant.loadingTitle", undefined, "Loading assistant...")}</strong>
        <span>{failed
          ? canRetry
            ? t("assistant.loadFailedBody", undefined, "The assistant interface could not load. Please try again.")
            : t("assistant.panel.loadUnavailable", undefined, "LAN still cannot be loaded. Your current work is preserved. Close this panel to continue, then reopen the app after resolving the loading issue.")
          : t("assistant.loadingBody", undefined, "The full assistant panel is loading.")}</span>
        {failed && canRetry ? <button type="button" className="assistant-load-retry" onClick={onRetry}>
          {t("assistant.retryLoad", undefined, "Try again")}
        </button> : null}
      </div>
    </div>
  );
}

function hasSameRect(current: DOMRect | null, next: DOMRect | null): boolean {
  return current === next || Boolean(
    current &&
    next &&
    current.left === next.left &&
    current.top === next.top &&
    current.width === next.width &&
    current.height === next.height
  );
}

export function AssistantCapsule(props: AssistantCapsuleProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [present, setPresent] = useState(false);
  const panelLoad = useDeferredModule(assistantPanelModule, present);
  const AssistantPanel = panelLoad.status === "ready" ? panelLoad.value : null;
  const [anchorRect, setAnchorRect] = useState<DOMRect | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const openRef = useRef(false);
  const focusFrame = useRef<number | null>(null);

  const updateAnchorRect = useCallback(() => {
    const next = buttonRef.current?.getBoundingClientRect() ?? null;
    setAnchorRect((current) => hasSameRect(current, next) ? current : next);
  }, []);

  const focusPanel = useCallback(() => {
    if (!openRef.current || props.confirmationOpen || surfaceRef.current?.dataset.state !== "open") return;
    const panel = panelRef.current;
    if (!panel) {
      return;
    }

    focusAssistantPanel(panel);
  }, [props.confirmationOpen]);

  const closePanel = useCallback(() => {
    openRef.current = false;
    updateAnchorRect();
    setOpen(false);
    if (focusFrame.current !== null) window.cancelAnimationFrame(focusFrame.current);
    focusFrame.current = window.requestAnimationFrame(() => {
      focusFrame.current = null;
      if (!openRef.current) buttonRef.current?.focus({ preventScroll: true });
    });
  }, [updateAnchorRect]);

  const finishExit = useCallback(() => {
    // An interrupted close must not remove a panel that has already reopened.
    if (!openRef.current) setPresent(false);
  }, []);

  useEffect(() => () => {
    if (focusFrame.current !== null) window.cancelAnimationFrame(focusFrame.current);
  }, []);

  useLayoutEffect(() => {
    if (!present) return;
    updateAnchorRect();
    // Geometry belongs to the whole presence lifetime, including interrupted exits.
    window.addEventListener("resize", updateAnchorRect);
    window.addEventListener("scroll", updateAnchorRect, true);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(updateAnchorRect);
    const button = buttonRef.current;
    if (button) observer?.observe(button);
    const header = button?.closest(".shell-header");
    if (header) observer?.observe(header);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", updateAnchorRect);
      window.removeEventListener("scroll", updateAnchorRect, true);
    };
  }, [present, updateAnchorRect]);

  useEffect(() => {
    if (!open || props.confirmationOpen) {
      return;
    }

    function handlePointerDown(event: MouseEvent) {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !surfaceRef.current?.contains(target)) {
        // Let the clicked control keep focus; only an explicit close returns to LAN.
        openRef.current = false;
        updateAnchorRect();
        setOpen(false);
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || event.isComposing) {
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        closePanel();
        return;
      }

      if (event.key !== "Tab" || !surfaceRef.current) {
        return;
      }

      containAssistantFocus(event, surfaceRef.current, document.activeElement);
    }

    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [closePanel, open, updateAnchorRect, props.confirmationOpen]);

  function handleCapsuleClick() {
    if (open) {
      closePanel();
      return;
    }

    updateAnchorRect();
    openRef.current = true;
    if (focusFrame.current !== null) window.cancelAnimationFrame(focusFrame.current);
    focusFrame.current = null;
    setPresent(true);
    setOpen(true);
  }

  const capsuleContent = <LanMark className="assistant-island-logo" />;

  return (
    <div
      ref={rootRef}
      className={open ? "assistant-shell is-open" : "assistant-shell"}
      data-no-window-drag="true"
    >
      <button
        ref={buttonRef}
        type="button"
        className={`assistant-capsule is-${props.assistant.tone}`}
        aria-expanded={open}
        aria-hidden={open || undefined}
        tabIndex={open ? -1 : 0}
        aria-haspopup="dialog"
        aria-label={props.assistant.panelTitle}
        title={props.assistant.panelTitle}
        onClick={handleCapsuleClick}
        onPointerEnter={preloadAssistantPanel}
        onFocus={preloadAssistantPanel}
      >
        {capsuleContent}
      </button>

      {present && typeof document !== "undefined" ? createPortal(
        <AssistantIslandSurface open={open} anchorRect={anchorRect} onExited={finishExit} surfaceRef={surfaceRef}
          onEntered={focusPanel} onClose={closePanel} closeLabel={t("assistant.closeLabel", undefined, "Close LAN")}
          panelTitle={props.assistant.panelTitle} originContent={capsuleContent}>
        {AssistantPanel ? (
          <AssistantPanel
            embedded
            aiSettings={props.aiSettings}
            assistantInput={props.assistantInput}
            draft={props.assistantDraft}
            execution={props.execution}
            anchorRect={anchorRect}
            panelRef={panelRef}
            messages={props.messages}
            conversations={props.conversations}
            activeConversationId={props.activeConversationId}
            onAction={props.onAction}
            onClearAiSecret={props.onClearAiSecret}
            onClose={closePanel}
            onDeleteConversation={props.onDeleteConversation}
            onDraftChange={props.onDraftChange}
            onNewConversation={props.onNewConversation}
            onSelectConversation={props.onSelectConversation}
            onRunPrompt={props.onRunPrompt}
            onReady={focusPanel}
            onSaveAiSettings={props.onSaveAiSettings}
            recoveryPending={props.recoveryPending}
            continuation={props.continuation}
            onResume={props.onResume}
            onStop={props.onStop}
            onSendMessage={props.onSendMessage}
          />
        ) : (
          <AssistantPanelFallback
            panelTitle={props.assistant.panelTitle}
            panelRef={panelRef}
            anchorRect={anchorRect}
            failed={panelLoad.status === "error"}
            canRetry={panelLoad.canRetry}
            onReady={focusPanel}
            onClose={closePanel}
            onRetry={panelLoad.retry}
          />
        )}
        </AssistantIslandSurface>,
        document.body
      ) : null}
    </div>
  );
}
