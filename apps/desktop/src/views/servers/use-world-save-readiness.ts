import { useEffect, useState } from "react";
import { readInstanceWorldSaveReadiness } from "../../api";
import { selectLocaleText } from "../../i18n-config";
import type { InstanceActiveRun, InstanceDetails, InstanceSummary, InstanceWorldSaveReadiness } from "../../types";

type Run = Pick<InstanceActiveRun, "run_id" | "session_id">;

export function worldSaveReadinessReason(value: InstanceWorldSaveReadiness | null, run: Run | null | undefined, locale: string): string | null {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  if (value && run && (value.run_id !== run.run_id || value.session_id !== (run.session_id ?? null))) {
    return text("服务器运行已变化，请刷新实例后重试。", "The server run has changed. Refresh the instance and try again.");
  }
  if (!value || value.prepared === null) {
    return text("无法确认本次运行的保存能力。请正常停止并重新启动服务器。", "Save readiness for this run is unknown. Stop the server normally and start it again.");
  }
  return value.prepared ? null : text(
    "本次运行未准备保存扩展，手动保存不可用。请正常停止服务器，更新程序后重新启动。",
    "The save extension was not prepared for this run. Manual saving is unavailable. Stop the server normally, update the app, and start it again.");
}

export async function requireWorldSaveReadiness(details: InstanceDetails, locale: string): Promise<void> {
  const current = await readInstanceWorldSaveReadiness(details.summary.id);
  const reason = worldSaveReadinessReason(current, details.active_run, locale);
  if (reason) throw new Error(reason);
}

/** Refresh with the existing instance read model; no timer or file inspection. */
export function useWorldSaveReadiness(instance: InstanceSummary, run: Run | null | undefined, enabled: boolean, locale: string): string | null {
  const runId = run?.run_id;
  const session = run?.session_id ?? null;
  const scope = `${instance.id}:${runId ?? ""}:${session}`;
  const [state, setState] = useState<{ scope: string; value: InstanceWorldSaveReadiness | null; error: boolean } | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void readInstanceWorldSaveReadiness(instance.id).then(
      (value) => { if (!cancelled) setState({ scope, value, error: false }); },
      () => { if (!cancelled) setState({ scope, value: null, error: true }); },
    );
    return () => { cancelled = true; };
  }, [instance, scope, enabled]);
  if (!enabled) return null;
  if (state?.scope !== scope) return selectLocaleText(locale, "正在检查本次运行的保存能力…", "Checking save readiness for this run…");
  if (state.error) return selectLocaleText(locale, "无法读取保存能力，请检查连接后刷新实例。", "Unable to read save readiness. Check the connection and refresh the instance.");
  return worldSaveReadinessReason(state.value, run, locale);
}
