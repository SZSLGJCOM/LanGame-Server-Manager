import { useEffect, useRef, useState } from "react";
import { readInstanceDetails, readModuleDetails, sendInstanceRuntimeCommand } from "../../api";
import { describeError } from "../../app-state";
import type { UiMessage } from "../../app-ui";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { InstanceSummary } from "../../types";
import { requestImmediateWorldSave, worldSaveDescription, worldSaveTitle, worldSaveResultMessage } from "./world-save-action";
import { requireWorldSaveReadiness, useWorldSaveReadiness } from "./use-world-save-readiness";

interface Props {
  instance: InstanceSummary;
  supported: boolean;
  disabled?: boolean;
  onActivity: (value: UiMessage) => void;
}

export function ServerCardWorldSave({ instance, supported, disabled, onActivity }: Props) {
  const { locale } = useI18n();
  const mounted = useRef(false);
  const inFlight = useRef(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ text: string; failed: boolean } | null>(null);
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const preparationReason = useWorldSaveReadiness(instance, undefined, supported && !disabled && instanceHasRunningProcess(instance), locale);
  if (!supported) return null;
  const unavailable = disabled ? text("请等待当前实例操作完成。", "Wait for the current instance operation to finish.")
    : !instanceHasRunningProcess(instance) ? text("请先启动服务器，再保存当前世界。", "Start the server before saving the current world.") : preparationReason;
  const label = worldSaveTitle(instance.module_id, locale);

  async function save() {
    if (unavailable || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setStatus(null);
    try {
      const [details, module] = await Promise.all([readInstanceDetails(instance.id), readModuleDetails(instance.module_id, { includePreservedProgramCounts: false })]);
      if (!mounted.current) return;
      if (details.summary.id !== instance.id || details.summary.module_id !== instance.module_id) {
        throw new Error("World-save instance identity mismatch");
      }
      await requireWorldSaveReadiness(details, locale);
      if (!mounted.current) return;
      const result = await requestImmediateWorldSave(details, module, locale, sendInstanceRuntimeCommand);
      if (!mounted.current) return;
      const summary = worldSaveResultMessage(result, locale);
      const response = [summary, result.response_text?.replace(/\s+/g, " ").trim()].filter(Boolean).join(" · ");
      setStatus({ text: response, failed: false });
      onActivity({ key: "activity.worldSaveResult", params: { name: instance.name, message: response },
        fallback: `${instance.name}: ${response}` });
    } catch (error) {
      if (!mounted.current) return;
      const detail = describeError(error);
      setStatus({ text: detail, failed: true });
      onActivity({ key: "activity.worldSaveResult", params: { name: instance.name, message: detail },
        fallback: `${instance.name}: ${detail}`, tone: "error" });
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return <>
    <button type="button" className={`secondary-button server-list-card-icon-action server-list-card-save-action${status?.failed ? " is-error" : ""}`}
      disabled={busy || Boolean(unavailable)} aria-busy={busy} aria-label={`${label}: ${instance.name}`}
      title={busy ? text("正在保存…", "Saving…") : unavailable || status?.text || `${label} · ${worldSaveDescription(instance.module_id, locale)}`} onClick={() => void save()}>
      <ShellIcon name={busy ? "loader" : status?.failed ? "alert-circle" : "database"} className="server-list-card-action-icon" />
    </button>
    <span className="sr-only" role={status?.failed ? "alert" : "status"}>{status?.text ?? ""}</span>
  </>;
}
