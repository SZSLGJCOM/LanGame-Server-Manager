import { useState } from "react";
import { useI18n } from "../i18n";
import type { BackgroundJob } from "../types";
import { formatRuntimeStartWarning, runtimeStartJobNotices, runtimeStartWarningKey,
  type RuntimeStartNotice } from "../runtime-start-warning";
import { ActivityNotice } from "./ActivityNotice";

export function RuntimeStartWarnings({ jobs, manual }: { jobs: BackgroundJob[]; manual?: RuntimeStartNotice }) {
  const { t } = useI18n();
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(() => new Set());
  const notices = [...runtimeStartJobNotices(jobs), ...(manual ? [manual] : [])];
  const entries = new Map<string, { text: string; details: string }>();
  for (const notice of notices) {
    for (const warning of notice.warnings) {
      const id = `${notice.id}:${runtimeStartWarningKey(warning)}`;
      if (dismissed.has(id)) continue;
      const formatted = formatRuntimeStartWarning(t, warning, notice.label || notice.instanceId);
      const context = notice.instanceId ? t("runtime.startWarning.instance", { value: notice.instanceId }) : "";
      entries.set(id, { text: formatted.text, details: [context, formatted.details].filter(Boolean).join("\n") });
    }
  }
  return <>{[...entries].map(([id, entry]) => <ActivityNotice key={id} tone="warning" details={entry.details}
    onDismiss={() => setDismissed((current) => new Set([...current, id]))}>{entry.text}</ActivityNotice>)}</>;
}
