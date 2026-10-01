import { useEffect, useRef, useState } from "react";
import { readInstanceDetails } from "../../api";
import { describeError } from "../../app-state";
import { message, type UiMessage } from "../../app-ui";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { BindAddressCandidate, InstanceConnectionInfo, InstanceSummary } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { readPreferredJoinAddress, resolveInstanceConnection } from "../../instance-connections";

interface Props {
  instance: InstanceSummary;
  connection?: InstanceConnectionInfo;
  failed: boolean;
  candidates: BindAddressCandidate[];
  onRead: (connection: InstanceConnectionInfo) => void;
  onActivity: (value: UiMessage) => void;
}

export function ServerCardConnection(props: Props) {
  const { locale, t } = useI18n();
  const coordinator = useInstanceSettingsSaveCoordinator();
  const mounted = useRef(false);
  const inFlight = useRef(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ state: "copied" | "failed"; endpoint: string } | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const connection = props.connection;
  const endpoint = connection ? resolveInstanceConnection({
    summary: { ...props.instance, bind_ip: connection.bind_ip },
    ports: connection.ports,
    settings_json: connection.settings_json
  }, props.candidates, readPreferredJoinAddress(props.instance.id), locale, t) : null;
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const copied = result?.state === "copied" && result.endpoint === endpoint?.endpoint;
  const failed = result?.state === "failed";
  const unavailable = text("暂无可用连接地址", "No join address available");
  const status = busy ? text("正在复制…", "Copying…")
    : failed ? text("复制失败，请重试", "Copy failed. Try again")
      : copied ? text("已复制", "Copied") : "";
  const label = text("复制连接地址", "Copy join address");
  const summary = endpoint?.endpoint ?? (props.failed
    ? text("连接信息读取失败", "Connection unavailable")
    : connection ? unavailable : text("正在读取连接信息…", "Loading connection…"));
  const portSeparator = endpoint && endpoint.kind !== "relay" ? endpoint.endpoint.lastIndexOf(":") : -1;

  async function copy() {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setResult(null);
    try {
      await coordinator.flush(props.instance.id);
      const details = await readInstanceDetails(props.instance.id);
      if (!mounted.current) return;
      if (details.summary.id !== props.instance.id) throw new Error("Instance connection identity mismatch");
      const fresh = resolveInstanceConnection(details, props.candidates,
        readPreferredJoinAddress(props.instance.id), locale, t);
      props.onRead({ instance_id: details.summary.id, bind_ip: details.summary.bind_ip,
        ports: details.ports, settings_json: details.settings_json });
      if (!fresh) throw new Error(unavailable);
      if (!navigator.clipboard?.writeText) throw new Error(t("activity.clipboardUnsupported"));
      await navigator.clipboard.writeText(fresh.endpoint);
      if (mounted.current) setResult({ state: "copied", endpoint: fresh.endpoint });
    } catch (error) {
      if (mounted.current) {
        setResult({ state: "failed", endpoint: "" });
        props.onActivity(message("activity.copyInviteFailed", { message: describeError(error) }, { tone: "error" }));
      }
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <div className="server-list-card-network">
      <div className="server-list-card-meta" title={endpoint ? `${endpoint.label} · ${endpoint.endpoint}` : summary}>
        {portSeparator >= 0 ? <>
          <span className="server-list-card-address">{summary.slice(0, portSeparator + 1)}</span>
          <span>{summary.slice(portSeparator + 1)}</span>
        </> : summary}
      </div>
      <button type="button"
        className={`secondary-button server-list-card-icon-action server-list-card-copy-action${failed ? " is-error" : ""}`}
        disabled={busy || (!endpoint && !props.failed)}
        aria-busy={busy}
        aria-label={`${status || label}: ${props.instance.name}`}
        title={status || (endpoint ? `${label} · ${endpoint.label} · ${endpoint.endpoint}` : summary)}
        onClick={() => void copy()}
      >
        <ShellIcon name={busy ? "loader" : copied ? "check" : failed ? "alert-circle" : "copy"} className="server-list-card-action-icon" />
      </button>
      <span className="sr-only" role="status">{status}</span>
    </div>
  );
}
