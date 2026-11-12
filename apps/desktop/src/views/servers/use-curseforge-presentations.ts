import { useEffect, useMemo, useState } from "react";
import { resolveManualModReferences } from "../../api";
import type { ManualModReferenceItem, ManualModReferenceResolveResult } from "../../types";

type Presentations = Record<string, ManualModReferenceItem>;

function merge(current: Presentations, items: ManualModReferenceItem[]): Presentations {
  const next = { ...current };
  for (const item of items) {
    if (item.status !== "resolved" || !item.resolved_id) continue;
    const previous = next[item.resolved_id];
    next[item.resolved_id] = { ...item, title: item.title ?? previous?.title,
      icon_url: item.icon_url ?? previous?.icon_url };
  }
  return next;
}

/** Read public display metadata from the runtime cache; never open a website on list refresh. */
export function useCurseForgePresentations(instanceId: string, enabled: boolean, ids: string[],
  imported: ManualModReferenceResolveResult | null): Presentations {
  const idsKey = [...new Set(ids.filter((id) => /^[1-9]\d{0,9}$/.test(id)))].sort().join("\n");
  const [snapshot, setSnapshot] = useState<{ instanceId: string; items: Presentations }>({ instanceId, items: {} });
  const [recent, setRecent] = useState<{ instanceId: string; items: Presentations }>({ instanceId, items: {} });
  const currentImport = imported?.instance_id === instanceId && imported.module_id === "arksurvivalascended" ? imported : null;
  useEffect(() => {
    if (currentImport) setRecent((previous) => ({ instanceId,
      items: merge(previous.instanceId === instanceId ? previous.items : {}, currentImport.items) }));
  }, [instanceId, currentImport]);
  useEffect(() => {
    if (!enabled || !idsKey) return;
    let cancelled = false;
    // Omit moduleId deliberately: numeric IDs only need the runtime cache, not the native browser.
    void resolveManualModReferences(instanceId, idsKey.split("\n")).then((result) => {
      if (cancelled || result.instance_id !== instanceId || result.module_id !== "arksurvivalascended") return;
      setSnapshot({ instanceId, items: merge({}, result.items) });
    }).catch((error: unknown) => {
      if (!cancelled) console.warn("Could not read cached CurseForge display metadata", error);
    });
    return () => { cancelled = true; };
  }, [instanceId, enabled, idsKey]);
  return useMemo(() => enabled ? merge(merge(snapshot.instanceId === instanceId ? snapshot.items : {},
    Object.values(recent.instanceId === instanceId ? recent.items : {})), currentImport?.items ?? []) : {},
    [enabled, instanceId, snapshot, recent, currentImport]);
}
