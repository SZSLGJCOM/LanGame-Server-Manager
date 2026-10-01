import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { AssistantConversationSummary } from "../assistant-conversations";
import { useI18n } from "../i18n";
import { ShellIcon } from "./ShellIcon";
import { containAssistantFocus, listAssistantFocusableElements } from "./assistant-focus";

interface AssistantHistoryDrawerProps {
  activeConversationId: string | null;
  conversations: AssistantConversationSummary[];
  disabled: boolean;
  open: boolean;
  onClose: () => void;
  onDelete: (conversationId: string) => void;
  onNew: () => void;
  onSelect: (conversationId: string) => void;
}

interface DeleteFocusOrigin {
  conversationId: string;
  adjacentIds: string[];
}

export function AssistantHistoryDrawer(props: AssistantHistoryDrawerProps) {
  const { locale, t } = useI18n();
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);
  const drawerRef = useRef<HTMLElement | null>(null);
  const deleteFocusOriginRef = useRef<DeleteFocusOrigin | null>(null);
  const restoreConfirmationFocusRef = useRef<"cancel" | "delete" | null>(null);
  const pendingConversation = props.conversations.find(
    (conversation) => conversation.id === pendingDeleteId
  ) ?? null;

  useEffect(() => {
    if (!props.open) {
      setPendingDeleteId(null);
    }
  }, [props.open]);

  useLayoutEffect(() => {
    const drawer = drawerRef.current;
    if (!props.open || !drawer) {
      return;
    }

    const previousFocus = document.activeElement;
    (listAssistantFocusableElements(drawer)[0] ?? drawer).focus();
    return () => {
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) {
        previousFocus.focus();
      }
    };
  }, [props.open]);

  useLayoutEffect(() => {
    if (!props.open) {
      deleteFocusOriginRef.current = null;
      restoreConfirmationFocusRef.current = null;
      return;
    }

    const conversationRemoved = pendingDeleteId !== null && !pendingConversation;
    const reason = restoreConfirmationFocusRef.current ?? (conversationRemoved ? "delete" : null);
    if (!reason) {
      return;
    }

    const origin = deleteFocusOriginRef.current;
    deleteFocusOriginRef.current = null;
    restoreConfirmationFocusRef.current = null;
    if (conversationRemoved) {
      setPendingDeleteId(null);
    }

    const drawer = drawerRef.current;
    if (!drawer) {
      return;
    }

    // Resolve against the committed list: the removed row or its neighbors may have changed.
    const buttons = listAssistantFocusableElements(drawer);
    const originalDeleteButton = reason === "cancel"
      ? buttons.find((button) => button.classList.contains("assistant-history-delete")
        && button.dataset.conversationId === origin?.conversationId)
      : undefined;
    const choices = buttons.filter((button) => button.classList.contains("assistant-history-select")
      && button.dataset.conversationId !== origin?.conversationId);
    const neighbor = origin?.adjacentIds
      .map((id) => choices.find((button) => button.dataset.conversationId === id))
      .find((button) => button !== undefined);
    const closeButton = buttons.find((button) => button.classList.contains("assistant-history-close"));
    (originalDeleteButton ?? neighbor ?? choices[0] ?? closeButton ?? drawer).focus();
  }, [pendingConversation, pendingDeleteId, props.conversations, props.disabled, props.open]);

  function requestDelete(conversationId: string) {
    if (props.disabled) {
      return;
    }
    const index = props.conversations.findIndex((conversation) => conversation.id === conversationId);
    deleteFocusOriginRef.current = {
      conversationId,
      adjacentIds: [props.conversations[index + 1]?.id, props.conversations[index - 1]?.id]
        .filter((id): id is string => id !== undefined)
    };
    restoreConfirmationFocusRef.current = null;
    setPendingDeleteId(conversationId);
  }

  function cancelDelete() {
    restoreConfirmationFocusRef.current = "cancel";
    setPendingDeleteId(null);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.nativeEvent.isComposing) {
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (pendingDeleteId) {
        cancelDelete();
      } else {
        props.onClose();
      }
    } else if (event.key === "Tab" && drawerRef.current) {
      event.stopPropagation();
      containAssistantFocus(event, drawerRef.current, document.activeElement);
    }
  }

  if (!props.open) {
    return null;
  }

  function formatUpdatedAt(updatedAt: number): string {
    try {
      return new Intl.DateTimeFormat(locale, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit"
      }).format(new Date(updatedAt));
    } catch {
      return "";
    }
  }

  return (
    <>
      <button
        type="button"
        className="assistant-history-scrim"
        tabIndex={-1}
        aria-label={t("assistant.history.close")}
        onClick={props.onClose}
      />
      <aside
        ref={drawerRef}
        className="assistant-history-drawer"
        role="dialog"
        aria-modal="true"
        aria-label={t("assistant.history.title")}
        tabIndex={-1}
        onKeyDown={handleKeyDown}
      >
        <div className="assistant-history-head">
          <strong>{t("assistant.history.title")}</strong>
          <span>{props.conversations.length}</span>
          <button type="button" className="assistant-history-close" aria-label={t("assistant.history.close")} title={t("assistant.history.close")} onClick={props.onClose}>
            <ShellIcon name="x" />
          </button>
        </div>

        <div className="assistant-history-create">
          <button
            type="button"
            disabled={props.disabled}
            onClick={() => {
              props.onNew();
              props.onClose();
            }}
          >
            <ShellIcon name="plus" />
            <span>{t("assistant.history.new")}</span>
          </button>
        </div>

        <div className="assistant-history-list">
          {props.conversations.length === 0 ? (
            <div className="assistant-history-empty">
              <strong>{t("assistant.history.emptyTitle")}</strong>
              <span>{t("assistant.history.emptyBody")}</span>
            </div>
          ) : (
            props.conversations.map((conversation) => {
              const active = conversation.id === props.activeConversationId;
              return (
                <div
                  key={conversation.id}
                  className={active ? "assistant-history-item is-active" : "assistant-history-item"}
                >
                  <button
                    type="button"
                    className="assistant-history-select"
                    data-conversation-id={conversation.id}
                    aria-current={active ? "true" : undefined}
                    disabled={props.disabled}
                    onClick={() => {
                      props.onSelect(conversation.id);
                      props.onClose();
                    }}
                  >
                    <ShellIcon name="message-square" />
                    <span className="assistant-history-item-copy">
                      <strong>{conversation.title || t("assistant.history.untitled")}</strong>
                      <small>{formatUpdatedAt(conversation.updatedAt)}</small>
                    </span>
                  </button>
                  <button
                    type="button"
                    className="assistant-history-delete"
                    data-conversation-id={conversation.id}
                    aria-label={`${t("assistant.history.delete")}：${conversation.title}`}
                    title={t("assistant.history.delete")}
                    disabled={props.disabled}
                    onClick={() => requestDelete(conversation.id)}
                  >
                    <ShellIcon name="trash" />
                  </button>
                </div>
              );
            })
          )}
        </div>

        {pendingConversation ? (
          <div className="assistant-history-confirm">
            <span>{t("assistant.history.deleteConfirm", { title: pendingConversation.title })}</span>
            <div>
              <button type="button" onClick={cancelDelete}>
                {t("assistant.history.cancel")}
              </button>
              <button
                type="button"
                className="is-danger"
                disabled={props.disabled}
                onClick={() => {
                  if (props.disabled) {
                    return;
                  }
                  props.onDelete(pendingConversation.id);
                  restoreConfirmationFocusRef.current = "delete";
                  setPendingDeleteId(null);
                }}
              >
                {t("assistant.history.delete")}
              </button>
            </div>
          </div>
        ) : null}
      </aside>
    </>
  );
}
