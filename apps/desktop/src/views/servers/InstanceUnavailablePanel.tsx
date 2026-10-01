import { useState } from "react";
import { inspectInstanceRemoval } from "../../api-storage";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { formatInstancePanelError } from "../../instance-panel-loader";
import { InstanceIsolationPanel } from "./InstanceIsolationPanel";
import { formatInstanceRemovalPlan } from "./instance-removal-presentation";
import "./instance-unavailable.css";

interface InstanceUnavailablePanelProps {
  instanceId: string;
  error?: string | null;
  loading: boolean;
  deleteDisabled: boolean;
  onRetry: () => void;
  onDelete: (instanceId: string) => void | Promise<void>;
  onOpenLocalPath: (path: string) => void;
}

export function InstanceUnavailablePanel(props: InstanceUnavailablePanelProps) {
  const { t } = useI18n();
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const failed = Boolean(props.error) || !props.loading;

  return <div className="instance-unavailable" aria-busy={props.loading}>
    <div className="server-workspace-empty">
      <div className="server-workspace-empty-symbol">
        <ShellIcon name={failed ? "server" : "loader"} className="server-workspace-empty-icon" />
      </div>
      <h3 role="status">{t(failed ? "servers.unavailable.title" : "servers.loading.details")}</h3>
      {failed ? <>
        <p>{t("servers.unavailable.description")}</p>
        <div className="instance-unavailable-actions">
          <button type="button" className="secondary-button" disabled={props.loading} onClick={props.onRetry}>
            {t("common.retry")}
          </button>
          <InlineConfirmAction type="button" className="secondary-button danger"
            disabled={props.deleteDisabled} scopeKey={props.instanceId}
            confirmation={t("servers.details.deleteInstanceConfirm")}
            prepareConfirmation={async () => formatInstanceRemovalPlan(await inspectInstanceRemoval(props.instanceId), t)}
            confirmLabel={t("common.delete")} onConfirm={() => props.onDelete(props.instanceId)}>
            {t("servers.details.deleteInstance")}
          </InlineConfirmAction>
        </div>
      </> : null}
    </div>
    {failed ? <details className="instance-unavailable-diagnostics"
      onToggle={(event) => setDiagnosticsOpen(event.currentTarget.open)}>
      <summary>{t("servers.unavailable.diagnostics")}</summary>
      {diagnosticsOpen ? <>
        {props.error ? <p className="instance-unavailable-error">{formatInstancePanelError(props.error, t)}</p> : null}
        <InstanceIsolationPanel instanceId={props.instanceId} onOpenLocalPath={props.onOpenLocalPath} />
      </> : null}
    </details> : null}
  </div>;
}
