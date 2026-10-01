import { formatInstancePanelError, type InstancePanelLoadState, type InstancePanelPart } from "../../instance-panel-loader";
import { useI18n } from "../../i18n";
import { ActivityNotice } from "../../components/ActivityNotice";

interface InstancePanelReadStatusProps {
  state?: InstancePanelLoadState | null;
  part: InstancePanelPart;
  loading?: boolean;
  error?: string | null;
  onRetry: () => void;
  placeholder?: boolean;
}

export function InstancePanelReadStatus(props: InstancePanelReadStatusProps) {
  const { t } = useI18n();
  const error = props.state?.errors[props.part] ?? props.error;
  const loading = props.state?.pending.includes(props.part) ?? props.loading;
  if (!error && !loading) return null;
  const label = t(`servers.loading.part.${props.part}`);
  if (!error && props.placeholder) return <p className="form-note" role="status">{t("servers.loading.pending", { part: label })}</p>;
  return <ActivityNotice tone={error ? "error" : "info"} action={error ?
    <button type="button" className="secondary-button" disabled={loading} onClick={props.onRetry}>{t("common.retry")}</button> : undefined}>
    {error ? `${t("servers.loading.failed", { part: label })} ${formatInstancePanelError(error, t)}`
      : t("servers.loading.pending", { part: label })}
  </ActivityNotice>;
}
