import { useId, useRef, useState } from "react";
import { updateInstanceProgram } from "../../api";
import { describeError } from "../../app-state";
import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { useI18n } from "../../i18n";
import type { BackgroundJob, InstanceDetails, InstanceIsolationReport } from "../../types";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { parseSettingsObject } from "../settings/guided-settings";
import { readProgramUpdatePolicy } from "./program-update-policy";

interface Props {
  details: InstanceDetails;
  report: InstanceIsolationReport;
  jobs: BackgroundJob[];
  onChanged: () => void;
}

export function InstanceProgramMaintenance({ details, report, jobs, onChanged }: Props) {
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
  const disabled = busy || !stopped || report.mode === "damaged" || Boolean(policyIssue);
  const help = useConfigurationFieldHelp(useId(), policyIssue ?? (!stopped ? t("servers.program.stopFirst") : undefined),
    undefined, undefined, "instructions");

  async function run(validate: boolean) {
    if (inFlight.current || disabled) return;
    inFlight.current = true;
    setPending(true);
    setMessage(null);
    setFailure(null);
    try {
      await updateInstanceProgram(details.summary.id, validate);
      setMessage(t("servers.program.completed"));
      onChanged();
    } catch (error) {
      setFailure({ message: describeError(error), validate });
    } finally {
      setPending(false);
      inFlight.current = false;
    }
  }

  return <>
    <div className="button-row" aria-busy={busy} ref={help.anchorRef} {...help.interactionProps}
      role={help.descriptionId ? "group" : undefined} tabIndex={help.descriptionId ? 0 : undefined}
      aria-describedby={help.descriptionId}>
      {help.helpNode}
      {[false, true].map((validate) => <InlineConfirmAction key={String(validate)}
        className="secondary-button" scopeKey={details.summary.id} disabled={disabled}
        aria-describedby={help.descriptionId}
        confirmation={t(shared ? "servers.program.confirmShared" : "servers.program.confirmIndependent")}
        onConfirm={() => run(validate)}>
        {t(validate ? "servers.program.validate" : "servers.program.update")}
      </InlineConfirmAction>)}
    </div>
    {pending ? <ActivityNotice>{t("servers.program.pending")}</ActivityNotice> : null}
    {message ? <ActivityNotice tone="success">{message}</ActivityNotice> : null}
    {failure ? <ActivityNotice tone="error" onDismiss={() => setFailure(null)} action={
      <button type="button" className="secondary-button" disabled={disabled}
        onClick={() => void run(failure.validate)}>{t("common.retry")}</button>}>
      {failure.message}
    </ActivityNotice> : null}
  </>;
}
