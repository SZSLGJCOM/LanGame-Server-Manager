import { useRef, useState } from "react";
import { updateInstanceProgram, updateAuthenticatedInstanceProgram } from "../../api";
import { formatDesktopError } from "../../desktop-error-message";
import { runSteamInstallationWithAccount, type SteamInstallationAccountContext } from "../../steam-installation-account";
import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { BackgroundJob, InstanceDetails, InstanceIsolationReport } from "../../types";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import { parseSettingsObject } from "../settings/guided-settings";
import { readProgramUpdatePolicy } from "./program-update-policy";

interface Props {
  details: InstanceDetails;
  report: InstanceIsolationReport;
  jobs: BackgroundJob[];
  onChanged: () => void;
  account?: SteamInstallationAccountContext;
  accountBusy?: boolean;
}

export function InstanceProgramMaintenance({ details, report, jobs, onChanged, account, accountBusy }: Props) {
  const { t } = useI18n();
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ message: string; validate: boolean } | null>(null);
  const inFlight = useRef(false);
  const shared = report.mode === "shared";
  const busy = pending || jobs.some((job) => (job.target_id === details.summary.id || shared && job.target_id === details.summary.module_id)
    && ["Pending", "Running"].includes(job.status));
  const stopped = details.summary.status === "Stopped" && details.summary.active_process_count === 0;
  let policyIssue: string | undefined;
  const settings = parseSettingsObject(details.settings_json, t);
  if (!settings.value) policyIssue = settings.error ?? t("servers.programPolicy.invalid");
  else {
    try { if (readProgramUpdatePolicy(settings.value) === "pinned") policyIssue = t("servers.programPolicy.manualPinned"); }
    catch { policyIssue = t("servers.programPolicy.invalid"); }
  }
  const disabled = busy || accountBusy || !stopped || report.mode === "damaged" || Boolean(policyIssue);
  const reason = policyIssue ?? (!stopped ? t("servers.program.stopFirst") : null);

  async function run(validate: boolean) {
    if (inFlight.current || disabled) return;
    inFlight.current = true;
    setPending(true);
    setMessage(null);
    setFailure(null);
    try {
      await runSteamInstallationWithAccount(() => updateInstanceProgram(details.summary.id, validate),
        (accountName) => updateAuthenticatedInstanceProgram(details.summary.id, validate, accountName), account);
      setMessage(t("servers.program.completed"));
      onChanged();
    } catch (error) {
      setFailure({ message: formatDesktopError(t, error), validate });
    } finally {
      setPending(false);
      inFlight.current = false;
    }
  }

  return <>
    {[false, true].map((validate) => {
      const label = t(validate ? "servers.program.validate" : "servers.program.update");
      return <ConfigurationActionHelp key={String(validate)} reason={disabled ? reason : null}>
        <InlineConfirmAction className="ghost-button maintenance-action maintenance-action--icon instance-program-action"
          aria-label={label} title={label} aria-busy={busy || undefined} scopeKey={details.summary.id} disabled={disabled}
          confirmation={t(shared ? "servers.program.confirmShared" : "servers.program.confirmIndependent")} confirmLabel={label}
          onConfirm={() => run(validate)}>
          <ShellIcon name={validate ? "check-circle" : "download"} className="maintenance-action__icon" aria-hidden="true" />
        </InlineConfirmAction>
      </ConfigurationActionHelp>;
    })}
    {pending ? <ActivityNotice>{t("servers.program.pending")}</ActivityNotice> : null}
    {message ? <ActivityNotice tone="success">{message}</ActivityNotice> : null}
    {failure ? <ActivityNotice tone="error" onDismiss={() => setFailure(null)} action={
      <button type="button" className="secondary-button" disabled={disabled}
        onClick={() => void run(failure.validate)}>{t("common.retry")}</button>}>
      {failure.message}
    </ActivityNotice> : null}
  </>;
}
