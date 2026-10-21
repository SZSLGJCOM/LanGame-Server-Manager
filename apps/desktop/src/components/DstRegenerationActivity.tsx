import { useRef, useState } from "react";
import { dstRegenerationJobPhase, dstRegenerationOutcomeKey, dstRegenerationPhaseLabel, selectFinishedDstRegenerationJob } from "../dst-regeneration-job";
import { selectLocaleText } from "../i18n";
import type { BackgroundJob } from "../types";
import { ActivityNoticeContent } from "./ActivityNotice";
import { ActivityProgress } from "./ActivityProgress";

/** A running DST world regeneration in the shared activity bar; steps are coarse, so the meter stays indeterminate. */
export function DstRegenerationActivity({ job, locale }: { job: BackgroundJob; locale: string }) {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  return <ActivityProgress active percent={null}
    label={text(`重新生成世界 · ${job.label}`, `Regenerating world · ${job.label}`)}
    detail={dstRegenerationPhaseLabel(dstRegenerationJobPhase(job) ?? "stopping", text)} />;
}

/** The outcome of a regeneration watched this session, until dismissed. */
export function useFinishedDstRegeneration(jobs: BackgroundJob[]) {
  const seen = useRef(new Set<string>());
  const [dismissed] = useState(() => new Set<string>());
  const [, rerender] = useState(0);
  const job = selectFinishedDstRegenerationJob(jobs, seen.current, dismissed);
  const dismiss = () => { if (job) { dismissed.add(dstRegenerationOutcomeKey(job)); rerender(value => value + 1); } };
  return { job, dismiss };
}

export function DstRegenerationOutcome({ job, locale, dismissLabel, onDismiss }: { job: BackgroundJob; locale: string; dismissLabel: string; onDismiss: () => void }) {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const phase = dstRegenerationJobPhase(job) ?? "failed";
  const tone = phase === "done" ? "success" : "error";
  return <ActivityNoticeContent tone={tone} dismissLabel={dismissLabel} onDismiss={onDismiss}>
    {`${text("重新生成世界", "Regenerate world")} · ${job.label} · ${dstRegenerationPhaseLabel(phase, text)}`}
  </ActivityNoticeContent>;
}
