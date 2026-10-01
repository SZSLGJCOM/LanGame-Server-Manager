import type { AssistantConversationState, AssistantLiveProgress, AssistantProgressSnapshot } from "./types";

const KINDS = new Set(["model_start", "text_delta", "phase", "tool_started", "tool_completed", "tool_failed"]);
const EMPTY: AssistantLiveProgress = { text: "", phase: "", tools: [], connectionIssue: false };

function validateProgress(value: AssistantProgressSnapshot): void {
  if (!Number.isSafeInteger(value.revision) || value.revision < 0 || !Number.isSafeInteger(value.cursor) || value.cursor < 0
    || typeof value.reset !== "boolean" || typeof value.text !== "string" || value.text.length > 65_536
    || !Array.isArray(value.events) || value.events.length > 256) throw new Error("Invalid assistant progress snapshot.");
  let cursor = -1;
  for (const event of value.events) {
    if (!Number.isSafeInteger(event.cursor) || event.cursor <= cursor || event.cursor > value.cursor
      || !KINDS.has(event.kind) || typeof event.text !== "string" || event.text.length > 65_536
      || (event.toolName !== null && (typeof event.toolName !== "string" || !/^[a-zA-Z0-9_-]{1,64}$/.test(event.toolName)))) {
      throw new Error("Invalid assistant progress event.");
    }
    cursor = event.cursor;
  }
}

/** One producer owns a turn. Closing it invalidates even an in-flight response. */
export class AssistantProgressPoller {
  private stopped = false;
  private started = false;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private cursor: number | undefined;
  private revision: number;
  private failures = 0;
  private value: AssistantLiveProgress = EMPTY;

  constructor(
    private readonly conversationId: string,
    minimumRevision: number,
    private readonly read: (cursor?: number) => Promise<AssistantConversationState>,
    private readonly publish: (progress: AssistantLiveProgress) => void,
    private readonly isCurrent: () => boolean,
  ) { this.revision = minimumRevision; }

  start(): void {
    if (this.started && !this.stopped) return;
    this.started = true;
    this.stopped = false;
    void this.poll(++this.generation);
  }

  stop(): void {
    this.stopped = true;
    this.generation++;
    if (this.timer !== undefined) clearTimeout(this.timer);
  }

  private active(generation: number): boolean { return !this.stopped && generation === this.generation && this.isCurrent(); }

  private async poll(generation: number): Promise<void> {
    if (!this.active(generation)) return;
    try {
      const state = await this.read(this.cursor);
      if (!this.active(generation)) return;
      if (state.conversationId !== this.conversationId) throw new Error("Progress belongs to another conversation.");
      const next = state.progress;
      if (next) {
        validateProgress(next);
        if (next.revision < this.revision || (next.revision === this.revision && this.cursor !== undefined && next.cursor < this.cursor)) {
          throw new Error("Stale assistant progress snapshot.");
        }
        // The preflight revision belongs to the previous completed user turn.
        if (next.revision > this.revision || this.cursor !== undefined) {
          const changedTurn = next.revision > this.revision;
          // A cursor gap makes previous tool/phase state incomplete; replay the retained snapshot.
          const rebuild = changedTurn || next.reset;
          let value = rebuild ? EMPTY : this.value;
          let tools = [...value.tools];
          for (const event of next.events) {
            if (!rebuild && this.cursor !== undefined && event.cursor <= this.cursor) continue;
            if (event.kind === "phase") value = { ...value, phase: event.text };
            if (event.kind === "model_start") value = { ...value, phase: "model_start" };
            if (event.kind === "tool_started" && event.toolName) {
              tools.push({ cursor: event.cursor, name: event.toolName, status: "running" });
            } else if ((event.kind === "tool_completed" || event.kind === "tool_failed") && event.toolName) {
              let index = tools.length - 1;
              while (index >= 0 && (tools[index].name !== event.toolName || tools[index].status !== "running")) index--;
              const tool = { cursor: event.cursor, name: event.toolName, status: event.kind === "tool_failed" ? "failed" as const : "completed" as const };
              if (index >= 0) tools[index] = tool; else tools.push(tool);
            }
          }
          this.value = { ...value, tools: tools.slice(-8), text: next.text, connectionIssue: false };
          this.revision = next.revision;
          this.cursor = next.cursor;
          this.publish(this.value);
        }
      }
      this.failures = 0;
    } catch {
      if (!this.active(generation)) return;
      this.failures = Math.min(this.failures + 1, 5);
      this.value = { ...this.value, connectionIssue: true };
      this.publish(this.value);
    }
    if (this.active(generation)) this.timer = setTimeout(() => { void this.poll(generation); }, Math.min(400 * 2 ** this.failures, 8_000));
  }
}
