import { useEffect, useRef } from "react";
import { listAssistantConversations, getAssistantConversationState } from "../api";
import { assistantConversationProviderIdentity } from "../assistant-conversations";
import { validateAssistantConversationState } from "../assistant-turn-control";
import type { AssistantConversationState, AssistantProviderSettingsInput, AssistantSavedConversation } from "../types";

export function useAssistantConversationRecovery(
  scopeKey: string,
  settings: AssistantProviderSettingsInput,
  enabled: boolean,
  restore: (identity: string, saved: AssistantSavedConversation, state: AssistantConversationState) => void,
  onError: (error: unknown) => void,
): void {
  const errorRef = useRef(onError);
  errorRef.current = onError;
  const identity = assistantConversationProviderIdentity(settings);
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    const provider = JSON.parse(identity) as [string, string, string];
    // State inspection only needs the provider binding, never a secret or client history.
    const binding = { provider: provider[0], model: provider[1], baseUrl: provider[2], apiKey: "" };
    void (async () => {
      const saved = await listAssistantConversations(binding);
      if (!Array.isArray(saved) || saved.length > 16) throw new Error("Invalid saved assistant conversation list.");
      for (const entry of saved) {
        if (!current) return;
        if (!entry || typeof entry.conversationId !== "string" || !entry.conversationId.trim()) throw new Error("Invalid saved assistant conversation identity.");
        const state = await getAssistantConversationState(entry.conversationId, binding);
        if (!current) return;
        validateAssistantConversationState(state, entry.conversationId);
        if (state.status !== "unavailable") restore(identity, entry, state);
      }
    })().catch((error: unknown) => { if (current) errorRef.current(error); });
    return () => { current = false; };
  }, [scopeKey, identity, enabled, restore]);
}
