import { useCallback, useMemo, useRef, useState } from "react";
import {
  appendAssistantConversationMessage,
  beginAssistantConversationTurn,
  bindAssistantBackendConversation,
  clearInsecureAssistantConversationStorage,
  createEmptyAssistantConversationStore,
  deleteAssistantConversation,
  listAssistantConversationSummaries,
  markAssistantConversationUnavailable,
  setAssistantConversationRecoveryPending,
  selectAssistantConversation,
  setAssistantConversationContinuation,
  restoreAssistantConversation,
  startNewAssistantConversation,
  type AssistantConversationStore
} from "../assistant-conversations";
import type { AssistantChatMessage, AssistantContinuation, AssistantConversationCreated, AssistantConversationState, AssistantSavedConversation } from "../types";

const EMPTY_MESSAGES: AssistantChatMessage[] = [];

export function useAssistantConversations(scopeKey: string) {
  const [store, setStore] = useState<AssistantConversationStore>(() => {
    clearInsecureAssistantConversationStorage();
    return createEmptyAssistantConversationStore();
  });
  const storeRef = useRef(store);

  const commit = useCallback((nextStore: AssistantConversationStore) => {
    storeRef.current = nextStore;
    setStore(nextStore);
  }, []);

  const currentStore = useCallback(() => storeRef.current, []);

  const restore = useCallback((identity: string, saved: AssistantSavedConversation, state: AssistantConversationState) => {
    commit(restoreAssistantConversation(currentStore(), scopeKey, identity, saved, state));
  }, [commit, currentStore, scopeKey]);

  const activeConversationId = store.activeConversationByScope[scopeKey] ?? null;
  const activeConversation = activeConversationId
    ? store.conversations.find((conversation) => conversation.id === activeConversationId) ?? null
    : null;
  const messages = activeConversation?.messages ?? EMPTY_MESSAGES;
  const conversations = useMemo(
    () => listAssistantConversationSummaries(store, scopeKey),
    [scopeKey, store]
  );

  const beginTurn = useCallback((userMessage: AssistantChatMessage, providerIdentity: string) => {
    const result = beginAssistantConversationTurn(currentStore(), scopeKey, userMessage, Date.now(), undefined, providerIdentity);
    commit(result.store);
    return {
      conversationId: result.conversationId,
      backendConversationId: result.backendConversationId,
      messages: result.messages
    };
  }, [commit, currentStore, scopeKey]);

  const bindBackend = useCallback((conversationId: string, binding: AssistantConversationCreated) => {
    commit(bindAssistantBackendConversation(currentStore(), conversationId, binding));
  }, [commit, currentStore]);

  const backendId = useCallback((conversationId: string) =>
    currentStore().conversations.find((item) => item.id === conversationId)?.backendConversationId ?? null,
  [currentStore]);

  const appendMessage = useCallback((conversationId: string, message: AssistantChatMessage) => {
    commit(appendAssistantConversationMessage(currentStore(), conversationId, message));
  }, [commit, currentStore]);

  const startNew = useCallback(() => {
    commit(startNewAssistantConversation(currentStore(), scopeKey));
  }, [commit, currentStore, scopeKey]);

  const select = useCallback((conversationId: string) => {
    commit(selectAssistantConversation(currentStore(), scopeKey, conversationId));
  }, [commit, currentStore, scopeKey]);

  const remove = useCallback((conversationId: string) => {
    const wasActive = currentStore().activeConversationByScope[scopeKey] === conversationId;
    commit(deleteAssistantConversation(currentStore(), scopeKey, conversationId));
    return wasActive;
  }, [commit, currentStore, scopeKey]);

  const setContinuation = useCallback((conversationId: string, continuation: AssistantContinuation | null) => {
    commit(setAssistantConversationContinuation(currentStore(), conversationId, continuation));
  }, [commit, currentStore]);

  const markUnavailable = useCallback((conversationId: string, unsentMessageId?: string) => {
    commit(markAssistantConversationUnavailable(currentStore(), conversationId, unsentMessageId));
  }, [commit, currentStore]);

  const setRecoveryPending = useCallback((conversationId: string, pending: boolean) => {
    commit(setAssistantConversationRecoveryPending(currentStore(), conversationId, pending));
  }, [commit, currentStore]);

  return {
    restore,
    markUnavailable,
    setRecoveryPending,
    activeConversationId,
    activeConversation,
    setContinuation,
    backendId,
    bindBackend,
    appendMessage,
    beginTurn,
    conversations,
    messages,
    remove,
    select,
    startNew
  };
}
