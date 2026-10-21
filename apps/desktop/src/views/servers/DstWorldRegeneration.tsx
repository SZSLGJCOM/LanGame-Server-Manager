import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { describeError } from "../../app-state";
import { canUseLocalDesktopApi } from "../../api-transport";
import { regenerateDstWorld } from "../../api-dst-world-regeneration";
import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { forgetDstMapCache } from "../../dst-map-cache";
import { selectLocaleText, useI18n } from "../../i18n";
import type { InstanceDetails } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";

/** No map is fresh until the regenerated world's run is known. */
export const DST_REGENERATION_PENDING_RUN = Number.MAX_SAFE_INTEGER;

export interface DstWorldRegenerationState { busy: boolean; freshFromRunId: number | null }
const IDLE: DstWorldRegenerationState = { busy: false, freshFromRunId: null };
// Session-wide, so leaving and reopening the map mid-regeneration keeps both the progress and the stale-map guard.
const states = new Map<string, DstWorldRegenerationState>();
const listeners = new Set<() => void>();
function publish(instanceId: string, state: DstWorldRegenerationState) {
  states.set(instanceId, state);
  for (const listener of listeners) listener();
}
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

export function useDstWorldRegeneration(instanceId: string): DstWorldRegenerationState {
  return useSyncExternalStore(subscribe, () => states.get(instanceId) ?? IDLE);
}

export function DstWorldRegeneration(props: { details: InstanceDetails }) {
  return canUseLocalDesktopApi() ? <LocalDstWorldRegeneration key={props.details.summary.id} {...props} /> : null;
}

/** One confirmed GM action button; the backend job reports every step in the activity bar. */
function LocalDstWorldRegeneration({ details }: { details: InstanceDetails }) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const coordinator = useInstanceSettingsSaveCoordinator();
  const instanceId = details.summary.id;
  const busy = useDstWorldRegeneration(instanceId).busy;
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);
  const [error, setError] = useState<string | null>(null);
  const reasonId = useId();

  async function regenerate() {
    publish(instanceId, { busy: true, freshFromRunId: DST_REGENERATION_PENDING_RUN }); setError(null);
    let freshFromRunId: number | null = null;
    try {
      await forgetDstMapCache(instanceId);
      await coordinator.flushBeforeStop(instanceId);
      await coordinator.flush(instanceId);
      freshFromRunId = (await regenerateDstWorld(instanceId)).run_id;
    } catch (cause) {
      if (mounted.current) setError(describeError(cause));
    } finally {
      // The old world's offline maps may have been written while it stopped; none of them is this world.
      // A cache failure must never leave the GM grid disabled.
      await forgetDstMapCache(instanceId).catch(() => undefined);
      publish(instanceId, { busy: false, freshFromRunId });
    }
  }
  const label = text("重新生成世界", "Regenerate world");
  const reason = busy ? text("世界操作进行中，进度见底部动态栏。", "A world operation is in progress; see the activity bar.") : null;
  // A grid cell: the disabled reason travels with the button (title + description).
  return <>
    <InlineConfirmAction className="secondary-button dst-gm-tool dst-gm-regenerate" wrapperClassName="dst-gm-confirm" disabled={busy}
      scopeKey={instanceId} aria-label={label} title={reason ?? label} {...(reason ? { "aria-describedby": reasonId } : {})}
      confirmation={text("重新生成世界将丢失当前世界，是否确认？", "Regenerating the world loses the current world. Continue?")}
      confirmLabel={text("确认", "Confirm")} onConfirm={() => { void regenerate(); }}>
      <ShellIcon name="refresh" /><span>{text("重抽", "Reroll")}</span>
      {reason ? <span id={reasonId} hidden>{reason}</span> : null}
    </InlineConfirmAction>
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
  </>;
}
