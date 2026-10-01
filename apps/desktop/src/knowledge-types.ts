export interface KnowledgeSettings { autoUpdate: boolean; intervalHours: number }
export interface KnowledgeProgress {
  phase: string; moduleId: string | null; sourceId: string | null;
  completed: number; total: number; downloadedBytes: number; totalDownloadBytes: number;
}
export interface KnowledgeSyncReport {
  startedAt: number; finishedAt: number; changedDocuments: number;
  sourcesSucceeded: number; sourcesFailed: number; cancelled: boolean; errors: string[];
}
export interface KnowledgeSource {
  id: string; title: string; authority: string; kind: string; url: string; state: string;
  documentCount: number; chunkCount: number;
  lastCheckedAt: number | null; lastSuccessAt: number | null; lastError: string | null;
}
export interface KnowledgeGame { moduleId: string; scope: string; gaps: string[]; sources: KnowledgeSource[] }
export interface KnowledgeSyncJob {
  id: string; moduleId: string | null;
  state: "running" | "cancelling" | "completed" | "partial" | "failed" | "cancelled" | "interrupted";
  startedAt: number; finishedAt: number | null; progress: KnowledgeProgress;
  report: KnowledgeSyncReport | null; error: string | null;
}
export interface KnowledgeRuntimeStatus {
  library: {
    settings: KnowledgeSettings;
    model: { id: string; revision: string; ready: boolean; downloadBytes: number };
    games: KnowledgeGame[]; lastRun: KnowledgeSyncReport | null;
  };
  job: KnowledgeSyncJob | null;
  schedulerError: string | null;
}
export interface KnowledgeApi {
  available(): boolean;
  status(): Promise<KnowledgeRuntimeStatus>;
  save(settings: KnowledgeSettings): Promise<void>;
  start(moduleId: string | null): Promise<KnowledgeSyncJob>;
  cancel(jobId: string): Promise<boolean>;
}

export function isKnowledgeJobActive(job: KnowledgeSyncJob | null | undefined): boolean {
  return job?.state === "running" || job?.state === "cancelling";
}
