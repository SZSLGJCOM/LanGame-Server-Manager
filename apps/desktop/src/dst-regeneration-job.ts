import type { BackgroundJob } from "./types";

/** Steps the backend publishes in a world-regeneration job's `detail`. */
const PHASES = ["stopping", "generating", "done", "failed"] as const;
export type DstRegenerationPhase = typeof PHASES[number];

const KIND = "RegenerateWorld";
const isRunning = (job: BackgroundJob) => ["running", "pending"].includes(job.status.toLowerCase());

export function dstRegenerationJobPhase(job: BackgroundJob): DstRegenerationPhase | null {
  return PHASES.find(value => value === job.detail) ?? null;
}

export function selectActiveDstRegenerationJob(jobs: BackgroundJob[]): BackgroundJob | null {
  return jobs.find(job => job.kind === KIND && isRunning(job) && dstRegenerationJobPhase(job) !== null) ?? null;
}

/** Key of one finished run of a job; a dismissed outcome stays hidden until that job runs again. */
export const dstRegenerationOutcomeKey = (job: BackgroundJob) => `${job.id}:${job.status}:${job.detail ?? ""}`;

/**
 * The finished regeneration to announce: only jobs this session watched
 * running, so outcomes from earlier sessions never reappear. `seen` is
 * updated in place with currently running jobs.
 */
export function selectFinishedDstRegenerationJob(jobs: BackgroundJob[], seen: Set<string>, dismissed: Set<string>): BackgroundJob | null {
  for (const job of jobs) {
    if (job.kind !== KIND || !isRunning(job)) continue;
    seen.add(job.id);
    for (const key of dismissed) if (key.startsWith(`${job.id}:`)) dismissed.delete(key);
  }
  return jobs.find(job => job.kind === KIND && !isRunning(job) && seen.has(job.id)
    && dstRegenerationJobPhase(job) !== null && !dismissed.has(dstRegenerationOutcomeKey(job))) ?? null;
}

export function dstRegenerationPhaseLabel(phase: DstRegenerationPhase, text: (zh: string, en: string) => string): string {
  switch (phase) {
    case "stopping": return text("正在停服", "Stopping the server");
    case "generating": return text("正在生成新世界", "Generating the new world");
    case "done": return text("新世界已生成", "The new world is ready");
    case "failed": return text("世界重新生成失败", "World regeneration failed");
  }
}
