import type { BackgroundJob, InstallProgress } from "./types";
import type { TranslateFn } from "./i18n";

export function isInstallationJob(job: BackgroundJob): boolean {
  return ["downloadgame", "validategame", "uninstallgame"].includes(job.kind.toLowerCase());
}

export function isActiveInstallationJob(job: BackgroundJob): boolean {
  return isInstallationJob(job) && ["pending", "running"].includes(job.status.toLowerCase());
}

export function selectActiveInstallationJob(jobs: BackgroundJob[]): BackgroundJob | null {
  const active = jobs.filter(isActiveInstallationJob);
  return active.find((job) => job.status.toLowerCase() === "running" && job.install_progress?.phase !== "queued")
    ?? active[0] ?? null;
}

export function installationJobPhase(job: BackgroundJob): InstallProgress["phase"] | "uninstalling" {
  if (job.status.toLowerCase() === "pending") return "queued";
  const phase = job.install_progress?.phase;
  if (phase && ["queued", "preparing", "downloading", "extracting", "installing", "verifying", "ready"].includes(phase)) return phase;
  return job.kind.toLowerCase() === "uninstallgame" ? "uninstalling"
    : job.kind.toLowerCase() === "validategame" ? "verifying" : "preparing";
}

export function installationJobPercent(job: BackgroundJob): number | null {
  const percent = job.install_progress?.percent;
  return typeof percent === "number" && Number.isFinite(percent) && percent >= 0
    ? Math.min(100, percent) : null;
}

export function installationJobPhaseLabel(job: BackgroundJob, t: TranslateFn): string {
  const phase = installationJobPhase(job);
  const detail = job.detail?.trim() ?? "";
  const retry = /^Retry ([1-4]): (.*)$/s.exec(detail);
  const label = installationStepLabel(phase, retry?.[2] ?? detail, t);
  return retry ? t("installation.retryStep", { attempt: retry[1], step: label }) : label;
}

function installationStepLabel(phase: ReturnType<typeof installationJobPhase>, detail: string, t: TranslateFn): string {
  // The provider owns measured phases and byte counts. Only describe its
  // current unmeasured step here; console text must not invent a percentage.
  if (phase === "preparing") {
    const steps: readonly [RegExp, string][] = [
      [/preallocating/i, "installation.step.allocating"],
      [/waiting for client config/i, "installation.step.clientConfig"],
      [/waiting for user info/i, "installation.step.userInfo"],
      [/waiting for app info|loading app info|requesting app info|\bapp_update \d+/i, "installation.step.appInfo"],
      [/connecting anonymously|connecting to steam|retrying.*connect/i, "installation.step.connecting"],
      [/logging in|login anonymous/i, "installation.step.signingIn"],
      [/missing configuration.*retry|transient file lock.*retry/i, "installation.step.retrying"],
      [/Steam Console Client|Loading Steam API/i, "installation.step.initializing"],
      [/SteamCMD:.*checking.*updates/i, "steamcmd.prepare.inspecting"],
      [/checking SteamCMD|inspecting.*SteamCMD|SteamCMD:.*verif/i, "steamcmd.prepare.verifying"],
      [/starting SteamCMD|SteamCMD ready/i, "installation.step.starting"],
    ];
    const step = steps.find(([pattern]) => pattern.test(detail));
    if (step) return t(step[1]);
  }
  if (phase === "installing") {
    if (/reconfiguring/i.test(detail)) return t("installation.step.configuring");
    if (/committing/i.test(detail)) return t("installation.step.committing");
  }
  return t(`installation.phase.${phase}`);
}

export function completedInstallationJob(previous: BackgroundJob[], current: BackgroundJob[]): BackgroundJob | null {
  const previousActiveIds = new Set(previous.filter(isActiveInstallationJob).map((job) => job.id));
  return current.find((job) => previousActiveIds.has(job.id)
    && ["completed", "failed", "cancelled"].includes(job.status.toLowerCase())) ?? null;
}
