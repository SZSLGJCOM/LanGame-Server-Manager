import { isTauri } from "@tauri-apps/api/core";
import { invokeOrMock, shouldUseLanApi } from "./api-transport";
import type { KnowledgeApi, KnowledgeRuntimeStatus, KnowledgeSettings, KnowledgeSyncJob } from "./knowledge-types";

export const isKnowledgeManagementAvailable = () => isTauri() || !shouldUseLanApi();

function invokeKnowledge<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isKnowledgeManagementAvailable()) return Promise.reject(new Error("Knowledge updates are available only on the desktop host."));
  return invokeOrMock<T>(command, args);
}

export const knowledgeApi: KnowledgeApi = {
  available: isKnowledgeManagementAvailable,
  status: () => invokeKnowledge<KnowledgeRuntimeStatus>("read_knowledge_status"),
  save: (input: KnowledgeSettings) => invokeKnowledge<void>("update_knowledge_settings", { input }),
  start: (moduleId) => invokeKnowledge<KnowledgeSyncJob>("start_knowledge_sync", { input: { moduleId, force: true } }),
  cancel: (jobId: string) => invokeKnowledge<boolean>("cancel_knowledge_sync", { input: { jobId } })
};
