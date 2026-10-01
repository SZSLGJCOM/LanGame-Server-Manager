import type { AssistantChatMessage, AssistantContinuation, AssistantConversationCreated, AssistantConversationState, AssistantProviderSettingsInput, AssistantSavedConversation } from "./types";

const STORE_VERSION = 1;
const MAX_CONVERSATIONS = 80;
const MAX_MESSAGES_PER_CONVERSATION = 120;
const MAX_MESSAGE_CONTENT_LENGTH = 32_000;
const MAX_TITLE_LENGTH = 42;
const INSECURE_ASSISTANT_CONVERSATION_STORAGE_KEY = "langame.assistant-conversations.v1:local";

export interface AssistantConversation {
  id: string;
  scopeKey: string;
  providerIdentity: string;
  backendConversationId: string | null;
  continuation: AssistantContinuation | null;
  unavailable: boolean;
  recoveryPending: boolean;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: AssistantChatMessage[];
}

export interface AssistantConversationSummary {
  id: string;
  title: string;
  updatedAt: number;
  messageCount: number;
}

export interface AssistantConversationStore {
  version: typeof STORE_VERSION;
  conversations: AssistantConversation[];
  activeConversationByScope: Record<string, string>;
}

export function createEmptyAssistantConversationStore(): AssistantConversationStore {
  return {
    version: STORE_VERSION,
    conversations: [],
    activeConversationByScope: {}
  };
}

/** Restored messages are display records. Only the backend owns model history. */
export function restoreAssistantConversation(
  store: AssistantConversationStore, scopeKey: string, identity: string,
  saved: AssistantSavedConversation, state: AssistantConversationState,
): AssistantConversationStore {
  if (state.conversationId !== saved.conversationId || state.status === "unavailable"
    || !Array.isArray(state.messages) || state.messages.length > MAX_MESSAGES_PER_CONVERSATION
    || !Number.isSafeInteger(saved.updatedAtUnixMs) || saved.updatedAtUnixMs < 0 || typeof saved.title !== "string"
    || state.messages.some((entry) => !["assistant", "user"].includes(entry.role) || typeof entry.content !== "string" || entry.content.length > 65_536)) {
    throw new Error("Invalid saved assistant conversation.");
  }
  // A response that arrives after the user started a turn must not replace it.
  if (store.conversations.some((item) => item.backendConversationId === saved.conversationId && item.providerIdentity === identity)) return store;
  const id = `restored-${saved.conversationId}`;
  return replaceConversation(store, {
    id, scopeKey, providerIdentity: identity, backendConversationId: saved.conversationId,
    title: saved.title, createdAt: saved.updatedAtUnixMs, updatedAt: saved.updatedAtUnixMs,
    continuation: state.continuation, unavailable: false, recoveryPending: state.status === "running",
    messages: state.messages.map((entry, index) => ({ id: `${id}-${index}`, ...entry, state: "ready" })),
  });
}

export function assistantConversationProviderIdentity(settings: AssistantProviderSettingsInput): string {
  return JSON.stringify([settings.provider.trim(), settings.model.trim(), settings.baseUrl.trim().replace(/\/+$/, "")]);
}

// Backend IDs are capabilities. This cache holds display state only and never
// supplies model history or requirements sources to the backend.
export function bindAssistantBackendConversation(
  store: AssistantConversationStore, conversationId: string, binding: AssistantConversationCreated,
): AssistantConversationStore {
  const conversation = store.conversations.find((item) => item.id === conversationId);
  if (!conversation) return store;
  if (!binding.conversationId.trim() || !Number.isSafeInteger(binding.revision) || binding.revision < 0) {
    throw new Error("The assistant returned an invalid conversation binding.");
  }
  if (conversation.backendConversationId && conversation.backendConversationId !== binding.conversationId) {
    throw new Error("The assistant conversation binding changed.");
  }
  return replaceConversation(store, { ...conversation, backendConversationId: binding.conversationId });
}

export function setAssistantConversationContinuation(
  store: AssistantConversationStore, conversationId: string, continuation: AssistantContinuation | null,
): AssistantConversationStore {
  const conversation = store.conversations.find((item) => item.id === conversationId);
  return conversation ? replaceConversation(store, { ...conversation, continuation }) : store;
}

export function markAssistantConversationUnavailable(
  store: AssistantConversationStore, conversationId: string, unsentMessageId?: string,
): AssistantConversationStore {
  const conversation = store.conversations.find((item) => item.id === conversationId);
  if (!conversation) return store;
  return replaceConversation(store, { ...conversation, unavailable: true, recoveryPending: false, continuation: null,
    // A failed preflight never sent this message; its new conversation owns it.
    messages: conversation.messages.filter((message) => message.id !== unsentMessageId) });
}

export function setAssistantConversationRecoveryPending(
  store: AssistantConversationStore, conversationId: string, recoveryPending: boolean,
): AssistantConversationStore {
  const conversation = store.conversations.find((item) => item.id === conversationId);
  return conversation ? replaceConversation(store, { ...conversation, recoveryPending }) : store;
}

function normalizeText(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.slice(0, maxLength) : "";
}

function normalizeMessage(value: unknown): AssistantChatMessage | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const candidate = value as Partial<AssistantChatMessage>;
  if (
    typeof candidate.id !== "string" ||
    (candidate.role !== "assistant" && candidate.role !== "user") ||
    typeof candidate.content !== "string"
  ) {
    return null;
  }

  const state =
    candidate.state === "ready" || candidate.state === "running" || candidate.state === "error"
      ? candidate.state
      : undefined;

  return {
    id: normalizeText(candidate.id, 160),
    role: candidate.role,
    content: normalizeText(candidate.content, MAX_MESSAGE_CONTENT_LENGTH),
    label: candidate.label == null ? null : normalizeText(candidate.label, 160),
    meta: candidate.meta == null ? null : normalizeText(candidate.meta, 600),
    state
  };
}

function normalizeConversation(value: unknown): AssistantConversation | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const candidate = value as Partial<AssistantConversation>;
  if (
    typeof candidate.id !== "string" ||
    typeof candidate.scopeKey !== "string" ||
    typeof candidate.createdAt !== "number" ||
    typeof candidate.updatedAt !== "number" ||
    !Array.isArray(candidate.messages)
  ) {
    return null;
  }

  return {
    id: normalizeText(candidate.id, 160),
    scopeKey: normalizeText(candidate.scopeKey, 500),
    providerIdentity: normalizeText(candidate.providerIdentity, 4096),
    unavailable: candidate.unavailable === true,
    recoveryPending: candidate.recoveryPending === true,
    continuation: candidate.continuation && typeof candidate.continuation.reason === "string"
      && typeof candidate.continuation.summary === "string" && typeof candidate.continuation.canResume === "boolean"
      ? { reason: candidate.continuation.reason, summary: candidate.continuation.summary, canResume: candidate.continuation.canResume } : null,
    backendConversationId: typeof candidate.backendConversationId === "string" ? normalizeText(candidate.backendConversationId, 160) : null,
    title: normalizeText(candidate.title, MAX_TITLE_LENGTH),
    createdAt: Number.isFinite(candidate.createdAt) ? candidate.createdAt : Date.now(),
    updatedAt: Number.isFinite(candidate.updatedAt) ? candidate.updatedAt : Date.now(),
    messages: candidate.messages
      .map(normalizeMessage)
      .filter((message): message is AssistantChatMessage => Boolean(message))
      .slice(-MAX_MESSAGES_PER_CONVERSATION)
  };
}

export function normalizeAssistantConversationStore(value: unknown): AssistantConversationStore {
  if (!value || typeof value !== "object") {
    return createEmptyAssistantConversationStore();
  }

  const candidate = value as Partial<AssistantConversationStore>;
  const conversations = Array.isArray(candidate.conversations)
    ? candidate.conversations
        .map(normalizeConversation)
        .filter((conversation): conversation is AssistantConversation => Boolean(conversation))
        .sort((left, right) => right.updatedAt - left.updatedAt)
        .slice(0, MAX_CONVERSATIONS)
    : [];
  const conversationIds = new Set(conversations.map((conversation) => conversation.id));
  const activeConversationByScope = Object.fromEntries(
    Object.entries(candidate.activeConversationByScope ?? {})
      .filter((entry): entry is [string, string] => typeof entry[1] === "string" && conversationIds.has(entry[1]))
      .map(([scopeKey, conversationId]) => [normalizeText(scopeKey, 500), conversationId])
  );

  return {
    version: STORE_VERSION,
    conversations,
    activeConversationByScope
  };
}

export function clearInsecureAssistantConversationStorage(): void {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.removeItem(INSECURE_ASSISTANT_CONVERSATION_STORAGE_KEY);
    }
  } catch {
    // Browser storage restrictions must not prevent an in-memory assistant session.
  }
}

function createConversationId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `conversation-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function resolveConversationTitle(message: AssistantChatMessage): string {
  const source = (message.label || message.content).replace(/\s+/g, " ").trim();
  return source.slice(0, MAX_TITLE_LENGTH);
}

function replaceConversation(
  store: AssistantConversationStore,
  conversation: AssistantConversation
): AssistantConversationStore {
  return normalizeAssistantConversationStore({
    ...store,
    conversations: [
      conversation,
      ...store.conversations.filter((candidate) => candidate.id !== conversation.id)
    ]
  });
}

export function beginAssistantConversationTurn(
  store: AssistantConversationStore,
  scopeKey: string,
  userMessage: AssistantChatMessage,
  now = Date.now(),
  idFactory: () => string = createConversationId,
  providerIdentity = ""
): {
  store: AssistantConversationStore;
  conversationId: string;
  messages: AssistantChatMessage[];
  backendConversationId: string | null;
} {
  const activeId = store.activeConversationByScope[scopeKey];
  const activeConversation = store.conversations.find(
    (conversation) => conversation.id === activeId && conversation.scopeKey === scopeKey && conversation.providerIdentity === providerIdentity && !conversation.unavailable
  );
  const conversationId = activeConversation?.id ?? idFactory();
  const messages = [...(activeConversation?.messages ?? []), userMessage].slice(-MAX_MESSAGES_PER_CONVERSATION);
  const conversation: AssistantConversation = {
    id: conversationId,
    scopeKey,
    providerIdentity,
    backendConversationId: activeConversation?.backendConversationId ?? null,
    continuation: null,
    unavailable: false,
    recoveryPending: false,
    title: activeConversation?.title ?? resolveConversationTitle(userMessage),
    createdAt: activeConversation?.createdAt ?? now,
    updatedAt: now,
    messages
  };
  const nextStore = replaceConversation(store, conversation);

  return {
    store: {
      ...nextStore,
      activeConversationByScope: {
        ...nextStore.activeConversationByScope,
        [scopeKey]: conversationId
      }
    },
    conversationId,
    backendConversationId: conversation.backendConversationId,
    messages
  };
}

export function appendAssistantConversationMessage(
  store: AssistantConversationStore,
  conversationId: string,
  message: AssistantChatMessage,
  now = Date.now()
): AssistantConversationStore {
  const conversation = store.conversations.find((candidate) => candidate.id === conversationId);
  if (!conversation) {
    return store;
  }

  return replaceConversation(store, {
    ...conversation,
    updatedAt: now,
    messages: [...conversation.messages, message].slice(-MAX_MESSAGES_PER_CONVERSATION)
  });
}

export function startNewAssistantConversation(
  store: AssistantConversationStore,
  scopeKey: string
): AssistantConversationStore {
  const activeConversationByScope = { ...store.activeConversationByScope };
  delete activeConversationByScope[scopeKey];
  return { ...store, activeConversationByScope };
}

export function selectAssistantConversation(
  store: AssistantConversationStore,
  scopeKey: string,
  conversationId: string
): AssistantConversationStore {
  const exists = store.conversations.some(
    (conversation) => conversation.id === conversationId && conversation.scopeKey === scopeKey
  );
  if (!exists) {
    return store;
  }

  return {
    ...store,
    activeConversationByScope: {
      ...store.activeConversationByScope,
      [scopeKey]: conversationId
    }
  };
}

export function deleteAssistantConversation(
  store: AssistantConversationStore,
  scopeKey: string,
  conversationId: string
): AssistantConversationStore {
  const conversations = store.conversations.filter((conversation) => conversation.id !== conversationId);
  const activeConversationByScope = { ...store.activeConversationByScope };

  if (activeConversationByScope[scopeKey] === conversationId) {
    const nextConversation = conversations
      .filter((conversation) => conversation.scopeKey === scopeKey)
      .sort((left, right) => right.updatedAt - left.updatedAt)[0];
    if (nextConversation) {
      activeConversationByScope[scopeKey] = nextConversation.id;
    } else {
      delete activeConversationByScope[scopeKey];
    }
  }

  return { ...store, conversations, activeConversationByScope };
}

export function listAssistantConversationSummaries(
  store: AssistantConversationStore,
  scopeKey: string
): AssistantConversationSummary[] {
  return store.conversations
    .filter((conversation) => conversation.scopeKey === scopeKey)
    .sort((left, right) => right.updatedAt - left.updatedAt)
    .map((conversation) => ({
      id: conversation.id,
      title: conversation.title,
      updatedAt: conversation.updatedAt,
      messageCount: conversation.messages.length
    }));
}
