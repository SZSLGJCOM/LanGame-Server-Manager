import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject
} from "react";
import { getAiSettingsStatus, type AiSettings, type PersistAiSettings } from "../ai-settings";
import type { AssistantConversationSummary } from "../assistant-conversations";
import { buildAssistantViewModel } from "../assistant-state";
import type { AssistantActionId, AssistantBuildInput, AssistantPromptCard } from "../assistant-types";
import { useAssistantPromptRotation } from "../hooks/useAssistantPromptRotation";
import { useI18n } from "../i18n";
import type { AssistantChatMessage, AssistantExecutionState } from "../types";
import { AppAiSettingsCard } from "./AppAiSettingsCard";
import { PrivacyNotice } from "./PrivacyNotice";
import { KnowledgeSettingsCard } from "./KnowledgeSettingsCard";
import { AssistantHistoryDrawer } from "./AssistantHistoryDrawer";
import { LanMark } from "./LanMark";
import { ShellIcon } from "./ShellIcon";
import { assistantPanelStyle } from "./assistant-panel-position";

interface AssistantPanelProps {
  embedded?: boolean;
  aiSettings: AiSettings;
  anchorRect?: DOMRect | null;
  assistantInput: AssistantBuildInput;
  draft: string;
  execution: AssistantExecutionState;
  panelRef: RefObject<HTMLDivElement | null>;
  messages: AssistantChatMessage[];
  conversations: AssistantConversationSummary[];
  activeConversationId: string | null;
  onAction: (actionId: AssistantActionId) => void | Promise<void>;
  onClearAiSecret: PersistAiSettings;
  onClose: () => void;
  onDeleteConversation: (conversationId: string) => void;
  onDraftChange: (value: string) => void;
  onNewConversation: () => void;
  onSelectConversation: (conversationId: string) => void;
  onRunPrompt: (prompt: AssistantPromptCard) => void | Promise<void>;
  onReady: () => void;
  onSaveAiSettings: PersistAiSettings;
  continuation?: import("../types").AssistantContinuation | null;
  recoveryPending?: boolean;
  onResume?: () => void | Promise<void>;
  onStop?: () => void | Promise<void>;
  onSendMessage: (message: string) => void | Promise<void>;
}

type AssistantSurface = "chat" | "ai-settings" | "knowledge" | "privacy";

export function AssistantPanel(props: AssistantPanelProps) {
  const { t } = useI18n();
  const assistant = buildAssistantViewModel(props.assistantInput, t);
  const feedRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const followFeedRef = useRef(true);
  const feedContextRef = useRef({ conversationId: props.activeConversationId, surface: "chat" });
  const [showLatest, setShowLatest] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [surface, setSurface] = useState<AssistantSurface>("chat");
  const settingsOpen = surface === "ai-settings";
  const chatOpen = surface === "chat";
  const settingsLabel = t("assistant.panel.settingsLabel", undefined, "AI settings");
  const knowledgeLabel = t("assistant.panel.knowledgeLabel", undefined, "Game knowledge");
  const privacyLabel = t("assistant.panel.privacyLabel", undefined, "Privacy and data use");
  const surfaceLabel = settingsOpen ? settingsLabel : surface === "knowledge" ? knowledgeLabel : privacyLabel;
  const aiReady = getAiSettingsStatus(props.aiSettings).ready;
  const panelStyle = assistantPanelStyle(props.anchorRect);
  const panelClassName = panelStyle ? "assistant-panel assistant-panel--chat is-anchored" : "assistant-panel assistant-panel--chat";
  const primaryIssue = assistant.issues[0] ?? null;
  const primaryIssueAction = primaryIssue?.action ?? null;
  const copy = {
    assistantName: t("assistant.chat.assistantName"),
    inputPlaceholder: t("assistant.chat.inputPlaceholder"),
    userAvatar: t("assistant.chat.userAvatar"),
    greeting: t("assistant.chat.greeting"),
    sendLabel: t("assistant.chat.sendLabel"),
    runningLabel: t("assistant.chat.runningLabel"),
    jumpLabel: t("assistant.chat.jumpLabel")
  };
  const sendHint = props.execution.status === "running"
    ? t(props.execution.stopping ? "assistant.chat.stopping" : "assistant.chat.stop")
    : !aiReady
      ? t("assistant.run.notReady")
      : copy.sendLabel;
  const rotatingPrompt = useAssistantPromptRotation(
    assistant.prompts,
    historyOpen || props.draft.length > 0 || !chatOpen
  );
  const inputPlaceholder = rotatingPrompt?.preview ?? copy.inputPlaceholder;

  useLayoutEffect(() => {
    props.onReady();
  }, [props.onReady, surface]);

  useLayoutEffect(() => {
    const input = inputRef.current;
    if (input) {
      input.style.height = "auto";
      input.style.height = `${Math.min(input.scrollHeight, 120)}px`;
    }
  }, [props.draft, surface]);

  useEffect(() => {
    const previous = feedContextRef.current;
    if (previous.conversationId !== props.activeConversationId || previous.surface !== surface) {
      followFeedRef.current = true;
    }
    feedContextRef.current = { conversationId: props.activeConversationId, surface };
    if (!feedRef.current) {
      return;
    }
    const emptyConversation = props.messages.length === 0 && props.execution.status !== "running";
    if (followFeedRef.current || emptyConversation) {
      feedRef.current.scrollTop = emptyConversation ? 0 : feedRef.current.scrollHeight;
      setShowLatest(false);
    }
  }, [surface, props.activeConversationId, props.messages, props.execution.status, props.execution.promptLabel, props.execution.progress]);

  function trackFeedPosition() {
    const feed = feedRef.current;
    if (!feed) return;
    followFeedRef.current = feed.scrollHeight - feed.scrollTop - feed.clientHeight <= 48;
    setShowLatest(!followFeedRef.current && (props.messages.length > 0 || props.execution.status === "running"));
  }

  function showLatestMessages() {
    followFeedRef.current = true;
    if (feedRef.current) feedRef.current.scrollTop = feedRef.current.scrollHeight;
    setShowLatest(false);
  }

  function startConversation() {
    if (props.execution.status === "running") return;
    followFeedRef.current = true;
    setShowLatest(false);
    props.onNewConversation();
    inputRef.current?.focus();
  }

  function runAction(actionId: AssistantActionId) {
    if (actionId === "open-ai-settings") {
      showSurface("ai-settings");
      return;
    }

    void props.onAction(actionId);
  }

  function showSurface(next: AssistantSurface) {
    setHistoryOpen(false);
    setSurface(next);
  }

  function handleSubmit() {
    const nextMessage = props.draft.trim();
    if (!nextMessage || !aiReady || props.execution.status === "running") {
      return;
    }

    followFeedRef.current = true;
    props.onDraftChange("");
    void props.onSendMessage(nextMessage);
  }

  function handleTextareaKeyDown(event: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      handleSubmit();
    }
  }

  function renderAvatar(isUser: boolean) {
    if (isUser) {
      return <div className="assistant-chat-avatar is-user">{copy.userAvatar}</div>;
    }

    return (
      <div className="assistant-chat-avatar is-mascot">
        <LanMark className="assistant-avatar-mark" label={copy.assistantName} />
      </div>
    );
  }

  function renderTurn(
    role: AssistantChatMessage["role"],
    body: string,
    extras: { id?: string; meta?: string | null; stateClass?: string } = {}
  ) {
    const isUser = role === "user";
    return (
      <div key={extras.id} className={`assistant-chat-message is-${role}${extras.stateClass ?? ""}`}>
        {renderAvatar(isUser)}
        <div className="assistant-chat-main">
          {isUser ? null : (
            <div className="assistant-chat-message-head">
              <strong>{copy.assistantName}</strong>
            </div>
          )}
          <div className="assistant-chat-bubble">
            <div className="assistant-chat-message-body">{body}</div>
            {extras.meta ? <div className="assistant-chat-message-meta">{extras.meta}</div> : null}
          </div>
        </div>
      </div>
    );
  }

  function renderTyping() {
    const progress = props.execution.progress;
    const phase = progress?.phase;
    return (
      <div className="assistant-chat-message is-assistant is-running">
        {renderAvatar(false)}
        <div className="assistant-chat-main">
          <div className="assistant-chat-message-head">
            <strong>{copy.assistantName}</strong>
          </div>
          <div className="assistant-chat-bubble" role="status" aria-label={copy.runningLabel}>
            {progress?.text ? <div className="assistant-chat-message-body">{progress.text}</div> : null}
            {phase ? <p className="assistant-chat-message-meta">{t(`assistant.progress.${phase}`, undefined, phase)}</p> : null}
            {progress?.tools.length ? <ul className="assistant-progress-tools">
              {progress.tools.map((tool) => {
                const label = t(`assistant.tools.${tool.name}`, undefined, "");
                return <li key={tool.cursor} className={`is-${tool.status}`}>
                  <span>{label || t("assistant.progress.tool")}{!label ? <code> · {tool.name}</code> : null}</span>
                  <span>{t(`assistant.progress.${tool.status}`)}</span>
                </li>;
              })}
            </ul> : null}
            {progress?.connectionIssue ? <p className="assistant-chat-message-meta" role="alert">{t("assistant.progress.connectionIssue")}</p> : null}
            {!progress?.text && !phase && !progress?.tools.length ? <div className="assistant-chat-typing" aria-hidden="true">
              <span /><span /><span />
            </div> : null}
          </div>
        </div>
      </div>
    );
  }

  function renderMessage(message: AssistantChatMessage) {
    const stateClass = message.state ? ` is-${message.state}` : "";
    return renderTurn(message.role, message.content, {
      id: message.id,
      meta: message.meta,
      stateClass
    });
  }

  return (
    <div
      ref={props.panelRef}
      className={panelClassName}
      style={panelStyle}
      role={props.embedded ? "group" : "dialog"}
      aria-label={chatOpen ? assistant.panelTitle : surfaceLabel}
      aria-modal={props.embedded ? undefined : true}
      tabIndex={-1}
      data-no-window-drag="true"
    >
      <div className="assistant-panel-head">
        <div className="assistant-panel-identity">
          {!chatOpen ? (
            <button type="button" className="assistant-back-button" aria-label={t("assistant.panel.backToChat")} title={t("assistant.panel.backToChat")} onClick={() => showSurface("chat")}>
              <ShellIcon name="chevron-left" width={18} height={18} />
            </button>
          ) : <div className="assistant-conversation-controls">
            <button type="button" className="assistant-new-button" aria-label={t("assistant.history.new")} title={t("assistant.history.new")} disabled={props.execution.status === "running"} onClick={startConversation}>
              <ShellIcon name="plus" width={18} height={18} />
            </button>
            <button
              type="button"
              className={historyOpen ? "assistant-history-button is-open" : "assistant-history-button"}
              aria-expanded={historyOpen}
              aria-label={t("assistant.history.tooltip")}
              title={t("assistant.history.tooltip")}
              onClick={() => setHistoryOpen((current) => !current)}
            >
              <ShellIcon name="history" width={18} height={18} />
            </button>
            <button
              type="button"
              className="assistant-privacy-button"
              aria-label={privacyLabel}
              title={privacyLabel}
              onClick={() => showSurface("privacy")}
            >
              <ShellIcon name="shield" width={18} height={18} />
            </button>
          </div>}
          {!chatOpen ? (
            <div className="assistant-panel-heading">
              <div className="assistant-panel-title">{surfaceLabel}</div>
            </div>
          ) : null}
        </div>
        <div className="assistant-panel-controls">
          {chatOpen ? (
            <>
              <button
                type="button"
                className="assistant-settings-button"
                aria-label={settingsLabel}
                title={settingsLabel}
                onClick={() => showSurface("ai-settings")}
              >
                <ShellIcon name="settings" width={18} height={18} />
              </button>
              <button
                type="button"
                className="assistant-knowledge-button"
                aria-label={knowledgeLabel}
                title={knowledgeLabel}
                onClick={() => showSurface("knowledge")}
              >
                <ShellIcon name="database" width={18} height={18} />
              </button>
            </>
          ) : null}
          <button type="button" className="assistant-close-button" aria-label={assistant.closeLabel} onClick={props.onClose}>
            <ShellIcon name="x" className="assistant-close-icon" width={18} height={18} />
          </button>
        </div>
      </div>

      <AssistantHistoryDrawer
        activeConversationId={props.activeConversationId}
        conversations={props.conversations}
        disabled={props.execution.status === "running"}
        open={historyOpen}
        onClose={() => setHistoryOpen(false)}
        onDelete={props.onDeleteConversation}
        onNew={props.onNewConversation}
        onSelect={props.onSelectConversation}
      />

      {surface === "privacy" ? (
        <PrivacyNotice className="assistant-privacy-surface" />
      ) : settingsOpen ? (
        <div className="assistant-ai-settings-surface" aria-label={settingsLabel}>
          <AppAiSettingsCard
            settings={props.aiSettings}
            onClearSecret={props.onClearAiSecret}
            onSave={props.onSaveAiSettings}
          />
        </div>
      ) : surface === "knowledge" ? (
        <div className="assistant-knowledge-surface" aria-label={knowledgeLabel}>
          <KnowledgeSettingsCard modules={props.assistantInput.bootstrap.state.modules}
            preferredModuleId={props.assistantInput.activeView === "library"
              ? props.assistantInput.selectedModuleId
              : props.assistantInput.selectedInstanceDetails?.summary.module_id} />
        </div>
      ) : (
        <>
          <div className="assistant-chat-region">
            <div ref={feedRef} className="assistant-chat-feed" role="log" aria-live="polite" aria-relevant="additions text" onScroll={trackFeedPosition}>
              {renderMessage({
                id: "lan-greeting",
                role: "assistant",
                content: copy.greeting
              })}
              {props.messages.map(renderMessage)}

              {props.execution.status === "running" ? renderTyping() : null}
            </div>
            {showLatest ? <button type="button" className="assistant-chat-jump" onClick={showLatestMessages}><ShellIcon name="chevron-right" />{t("assistant.chat.latest")}</button> : null}
          </div>

          {(props.continuation || props.recoveryPending) && props.execution.status !== "running" ? (
            <section className="assistant-chat-notice" aria-label={t("assistant.run.pausedLabel")}>
              <div className="assistant-chat-notice-copy"><span>{props.recoveryPending ? t("assistant.session.unknown")
                : props.continuation?.reason === "investigation_failed" ? t("assistant.session.investigationFailed") : props.continuation?.summary}</span></div>
              {(props.recoveryPending || props.continuation?.canResume) && props.onResume ? (
                <button type="button" className="assistant-chat-notice-action" disabled={!aiReady} onClick={() => { void props.onResume?.(); }}>
                  {t(props.recoveryPending ? "assistant.session.check" : "assistant.run.resume")}
                </button>
              ) : null}
            </section>
          ) : null}

          {primaryIssue ? (
            <div className={`assistant-chat-notice is-${primaryIssue.severity}`}>
              <div className="assistant-chat-notice-copy">
                <strong>{primaryIssue.title}</strong>
                <span>{primaryIssue.detail}</span>
              </div>
              {primaryIssueAction ? (
                <button type="button" className="assistant-chat-notice-action" onClick={() => runAction(primaryIssueAction.id)}>
                  {primaryIssueAction.label || copy.jumpLabel}
                </button>
              ) : null}
            </div>
          ) : null}

          {props.assistantInput.storageReady ? <div className="assistant-chat-composer">
            <div className="assistant-chat-composer-shell">
              <textarea
                ref={inputRef}
                className="assistant-chat-input"
                value={props.draft}
                rows={1}
                placeholder={inputPlaceholder}
                aria-label={copy.inputPlaceholder}
                onChange={(event) => props.onDraftChange(event.target.value)}
                onKeyDown={handleTextareaKeyDown}
              />

              <span className="assistant-send-control" title={sendHint}>
                <button
                  type="button"
                  className="assistant-inline-button is-primary"
                  aria-label={sendHint}
                  disabled={props.execution.status === "running" ? props.execution.stopping === true || !props.onStop : !aiReady || !props.draft.trim()}
                  onClick={props.execution.status === "running" ? () => { void props.onStop?.(); } : handleSubmit}
                >
                  <ShellIcon name={props.execution.status === "running" ? "square" : "send"} />
                </button>
              </span>
            </div>
          </div> : null}
        </>
      )}
    </div>
  );
}
