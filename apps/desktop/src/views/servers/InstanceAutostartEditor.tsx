import { useEffect, useMemo, useRef, useState } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { TranslateFn } from "../../i18n";
import type { BackgroundJob, InstanceDetails } from "../../types";
import { formatInstanceAutostartJobDetail, isInstanceAutostartJob } from "../../instance-autostart-job";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";

interface InstanceAutostartEditorProps {
  details: InstanceDetails;
  readOnly?: boolean;
  t: TranslateFn;
  jobs?: readonly BackgroundJob[];
  onSaveAutostart?(instanceId: string, autostart: boolean): Promise<void>;
}

interface SaveScope { instanceId: string; saving: boolean }
interface AutostartDraft {
  scope: SaveScope;
  value: boolean;
  status: "idle" | "saving" | "saved" | "failed";
  error: string | null;
}

export function InstanceAutostartEditor(props: InstanceAutostartEditorProps) {
  const scope = useMemo<SaveScope>(
    () => ({ instanceId: props.details.summary.id, saving: false }),
    [props.details.summary.id]
  );
  const currentScope = useRef<SaveScope | null>(scope);
  currentScope.current = scope;
  const [draft, setDraft] = useState<AutostartDraft | null>(null);
  const activeDraft = draft?.scope === scope ? draft : null;
  const saving = activeDraft?.status === "saving";
  const checked = activeDraft && ["saving", "failed"].includes(activeDraft.status)
    ? activeDraft.value : props.details.summary.autostart;
  const latestJob = props.jobs?.find((job) => job.target_id === scope.instanceId && isInstanceAutostartJob(job));

  useEffect(() => {
    currentScope.current = scope;
    return () => { if (currentScope.current === scope) currentScope.current = null; };
  }, [scope]);

  async function save(autostart: boolean) {
    if (props.readOnly || !props.onSaveAutostart || scope.saving || currentScope.current !== scope) return;
    scope.saving = true;
    setDraft({ scope, value: autostart, status: "saving", error: null });
    try {
      // This mutation owns only autostart; concurrent configuration and backup saves remain independent.
      await props.onSaveAutostart(scope.instanceId, autostart);
      if (currentScope.current !== scope) return;
      setDraft({ scope, value: autostart, status: "saved", error: null });
    } catch (error) {
      if (currentScope.current !== scope) return;
      setDraft({ scope, value: autostart, status: "failed", error: normalizeConfigurationSaveError(error).message });
    } finally {
      scope.saving = false;
    }
  }

  return <div className="server-autostart-policy-editor">
    <label className="settings-toggle-card server-backup-policy-toggle">
      <span className="settings-toggle-copy">
        <span className="settings-field-title">{props.t("settings.details.autostart")}</span>
      </span>
      <input type="checkbox" checked={checked} disabled={saving || Boolean(props.readOnly)}
        onChange={(event) => void save(event.target.checked)} />
    </label>
    {activeDraft && activeDraft.status !== "idle" ? <ActivityNotice
      tone={activeDraft.status === "failed" ? "error" : saving ? "info" : "success"}
      action={activeDraft.status === "failed" ? <button type="button" className="secondary-button"
        onClick={() => void save(activeDraft.value)}>
        {props.t("settings.configuration.save.retry", undefined, "Retry save")}
      </button> : undefined}>
      {saving ? props.t("servers.backups.policySaving", undefined, "Saving policy…")
        : activeDraft.status === "saved" ? props.t("servers.backups.policySaved", undefined, "Policy saved")
          : props.t("servers.backups.policyError", { message: activeDraft.error ?? "" }, `Unable to save policy: ${activeDraft.error ?? ""}`)}
    </ActivityNotice> : null}
    {latestJob ? <ActivityNotice key={latestJob.id}
      tone={latestJob.status === "Failed" ? "error" : latestJob.status === "Completed" ? "success" : "info"}>
      {`${props.t(`status.job.${latestJob.status.toLowerCase()}`)} · ${formatInstanceAutostartJobDetail(latestJob, props.t)}`}
    </ActivityNotice> : null}
  </div>;
}
