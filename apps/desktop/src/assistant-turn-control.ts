import type { AssistantConversationControlResult, AssistantConversationState, AssistantExecuteOperationOutput } from "./types";

export function validateAssistantConversationState(state: AssistantConversationState, conversationId: string): void {
  if (state.conversationId !== conversationId || !["idle", "running", "paused", "unavailable"].includes(state.status)
    || (state.status !== "unavailable" && (!Number.isSafeInteger(state.revision) || state.revision === null || state.revision < 0))
    || (state.status === "paused" && (!state.continuation || typeof state.continuation.summary !== "string"
      || typeof state.continuation.canResume !== "boolean"))) {
    throw new Error("The assistant returned an invalid conversation state.");
  }
}

/** One UI request owns cancellation until its final backend response arrives. */
export class AssistantTurnControl {
  conversationId: string | null = null;
  stopRequested = false;
  private cancellation: Promise<AssistantConversationControlResult> | null = null;
  private revision = -1;

  constructor(private readonly cancel: (id: string) => Promise<AssistantConversationControlResult>) {}

  async bind(conversationId: string): Promise<void> {
    if (!conversationId.trim() || (this.conversationId && this.conversationId !== conversationId)) {
      throw new Error("The assistant returned a different conversation.");
    }
    this.conversationId = conversationId;
    if (this.stopRequested) await this.stop();
  }

  async stop(): Promise<AssistantConversationControlResult | null> {
    this.stopRequested = true;
    // Creating the backend session is short, but can still overlap a stop click.
    // bind() delivers that cancellation before any model request is sent.
    if (!this.conversationId) return null;
    if (!this.cancellation) {
      const id = this.conversationId;
      this.cancellation = this.cancel(id).then((result) => {
        if (result.conversationId !== id) throw new Error("Cancellation returned a different conversation.");
        return result;
      }).catch((error: unknown) => {
        this.cancellation = null;
        this.stopRequested = false;
        throw error;
      });
    }
    return this.cancellation;
  }

  async waitForCancellation(): Promise<void> {
    // The caller of stop() reports cancellation errors. This wait only prevents
    // a delayed cancellation from overlapping the next user turn.
    await this.cancellation?.then(() => undefined, () => undefined);
  }

  validate(output: AssistantExecuteOperationOutput): void {
    if (output.conversationId !== this.conversationId || !this.conversationId
      || !Number.isSafeInteger(output.conversationRevision)
      || output.conversationRevision === null || output.conversationRevision < 0 || output.conversationRevision < this.revision) {
      throw new Error("The assistant returned a stale or different conversation result.");
    }
    this.revision = output.conversationRevision;
  }
}
