export type AssistantConnectionStageStatus = "passed" | "failed" | "skipped";

export interface AssistantConnectionStage {
  status: AssistantConnectionStageStatus;
  diagnostic: string | null;
  latencyMs: number;
}

export interface AssistantConnectionCheckOutput {
  requestId: string;
  provider: string;
  model: string;
  endpointUrl: string;
  chat: AssistantConnectionStage;
  toolCall: AssistantConnectionStage;
  toolReplay: AssistantConnectionStage;
  elapsedMs: number;
  requestCount: number;
  cancelled: boolean;
}
