import { isTauri } from "@tauri-apps/api/core";
import type { AiSettings } from "./ai-settings";
import type { AssistantConnectionCheckOutput, AssistantConnectionStage } from "./assistant-connection-types";
import { invokeOrMock, shouldUseLanApi } from "./api-transport";

export function assistantConnectionCheckAvailable(): boolean {
  return isTauri() || shouldUseLanApi();
}

function requireConnectionHost() {
  if (!assistantConnectionCheckAvailable()) {
    throw new Error("Connection checks require the desktop host or an authenticated management connection.");
  }
}

function stage(value: unknown): value is AssistantConnectionStage {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return typeof record.status === "string" && ["passed", "failed", "skipped"].includes(record.status) &&
    (record.diagnostic === null || typeof record.diagnostic === "string" && record.diagnostic.length <= 2048) &&
    typeof record.latencyMs === "number" && Number.isSafeInteger(record.latencyMs) &&
    record.latencyMs >= 0 && record.latencyMs <= 120_000;
}

function publicStage(value: AssistantConnectionStage): AssistantConnectionStage {
  return { status: value.status, diagnostic: value.diagnostic, latencyMs: value.latencyMs };
}

function parseResult(value: unknown, settings: AiSettings, requestId: string): AssistantConnectionCheckOutput {
  if (!value || typeof value !== "object") throw new Error("Invalid connection-check response.");
  const record = value as Record<string, unknown>;
  let endpoint: URL;
  try {
    if (typeof record.endpointUrl !== "string") throw new Error();
    endpoint = new URL(record.endpointUrl);
  } catch {
    throw new Error("Invalid connection-check endpoint.");
  }
  if (record.requestId !== requestId || record.provider !== settings.provider ||
    record.model !== settings.model.trim() || !["http:", "https:"].includes(endpoint.protocol) ||
    endpoint.username || endpoint.password || endpoint.search || endpoint.hash ||
    !stage(record.chat) || !stage(record.toolCall) || !stage(record.toolReplay) ||
    typeof record.elapsedMs !== "number" || !Number.isSafeInteger(record.elapsedMs) ||
    record.elapsedMs < 0 || record.elapsedMs > 120_000 ||
    typeof record.requestCount !== "number" || !Number.isSafeInteger(record.requestCount) ||
    record.requestCount < 0 || record.requestCount > 3 || typeof record.cancelled !== "boolean") {
    throw new Error("Invalid connection-check response.");
  }
  // Preserve only the public contract: a response cannot introduce hidden
  // reasoning, raw provider envelopes or credentials into the settings view.
  return {
    requestId, provider: settings.provider, model: settings.model.trim(), endpointUrl: endpoint.toString(),
    chat: publicStage(record.chat), toolCall: publicStage(record.toolCall), toolReplay: publicStage(record.toolReplay),
    elapsedMs: record.elapsedMs, requestCount: record.requestCount, cancelled: record.cancelled
  };
}

export async function checkAssistantConnection(settings: AiSettings, requestId: string): Promise<AssistantConnectionCheckOutput> {
  requireConnectionHost();
  const result = await invokeOrMock<unknown>("assistant_check_connection", { input: {
    requestId,
    settings: { provider: settings.provider, model: settings.model.trim(), baseUrl: settings.baseUrl.trim(), apiKey: settings.apiKey }
  } });
  return parseResult(result, settings, requestId);
}

export async function cancelAssistantConnectionCheck(requestId: string): Promise<void> {
  requireConnectionHost();
  const result = await invokeOrMock<unknown>("assistant_cancel_connection_check", { requestId });
  if (typeof result !== "boolean") throw new Error("Invalid connection-check cancellation response.");
}
