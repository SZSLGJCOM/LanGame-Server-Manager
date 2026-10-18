import type { TranslateFn } from "./i18n";
import type { BackgroundJob } from "./types";
import { formatDesktopError } from "./desktop-error-message";
import { formatRuntimeStartWarning, readRuntimeStartWarnings } from "./runtime-start-warning";

export function isInstanceAutostartJob(job: BackgroundJob): boolean {
  return job.kind === "StartInstance" && job.label.startsWith("Autostart ");
}

export function formatInstanceAutostartJobLabel(job: BackgroundJob, t: TranslateFn): string {
  return isInstanceAutostartJob(job)
    ? t("servers.autostart.jobLabel", { name: job.label.slice("Autostart ".length) }) : job.label;
}

export function formatInstanceAutostartJobDetail(job: BackgroundJob, t: TranslateFn): string {
  const warnings = job.status === "Completed" ? readRuntimeStartWarnings(job.warnings) : [];
  if (warnings.length) return warnings.map((warning) => {
    const rendered = formatRuntimeStartWarning(t, warning, formatInstanceAutostartJobLabel(job, t));
    return `${rendered.text}\n${rendered.details}`;
  }).join("\n\n");
  if (isInstanceAutostartJob(job)) {
    switch (job.detail) {
      case "Starting automatically with LGSM.": return t("servers.autostart.starting");
      case "Started automatically with LGSM.": return t("servers.autostart.started");
      case "Autostart skipped because the instance is already running or starting.": return t("servers.autostart.skipped");
      case "Autostart cancelled.": return t("servers.autostart.cancelled");
    }
  }
  return job.detail ? formatDesktopError(t, job.detail) : job.output_excerpt ?? "";
}
