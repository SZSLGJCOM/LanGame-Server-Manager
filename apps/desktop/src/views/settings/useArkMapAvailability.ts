import { useEffect, useMemo, useRef, useState } from "react";
import { readInstanceArkMapAvailability } from "../../api";
import type { InstalledArkMap, InstanceArkMapAvailability } from "../../ark-map-availability";
import type { InstanceDetails, ModuleDetails } from "../../types";
import type { TranslateFn } from "../../i18n";
import { arkMapsText, mergeArkMapSuggestions, readArkMapSuggestions, type ArkMapAvailabilityView } from "./ark-cluster-maps";

interface MapLoadState {
  scope: object;
  revision: number;
  status: ArkMapAvailabilityView["status"];
  maps: readonly InstalledArkMap[];
  error: Error | null;
}

/** One read-only inventory request feeds both map controls; late replies never cross instance scopes. */
export function useArkMapAvailability(details: InstanceDetails, moduleDetails: ModuleDetails | null, locale: string,
  t: TranslateFn, enabled: boolean): ArkMapAvailabilityView | undefined {
  const supported = enabled && details.summary.module_id === "arksurvivalascended"
    && moduleDetails?.summary.id === "arksurvivalascended";
  const instanceId = details.summary.id;
  const scope = useMemo(() => ({}), [instanceId, details.settings_json, supported]);
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<MapLoadState | null>(null);
  const request = useRef<{ scope: object; revision: number; promise: Promise<InstanceArkMapAvailability> } | null>(null);
  const current = state?.scope === scope && state.revision === revision ? state : null;
  const status = current?.status ?? "loading";

  useEffect(() => {
    if (!supported) return;
    let cancelled = false;
    setState({ scope, revision, status: "loading", maps: [], error: null });
    if (request.current?.scope !== scope || request.current.revision !== revision) {
      request.current = { scope, revision, promise: readInstanceArkMapAvailability(instanceId) };
    }
    void request.current.promise.then((response) => {
      if (!cancelled) setState({ scope, revision, status: "ready", maps: response.maps, error: null });
    }).catch((cause: unknown) => {
      if (!cancelled) setState({ scope, revision, status: "failed", maps: [],
        error: cause instanceof Error ? cause : new Error(String(cause)) });
    });
    return () => { cancelled = true; };
  }, [instanceId, revision, scope, supported]);

  const choices = useMemo(() => supported ? mergeArkMapSuggestions(readArkMapSuggestions(moduleDetails?.schema_json, locale),
    (current?.maps ?? []).map((map) => ({ value: map.map_name, modId: map.mod_id,
      label: arkMapsText(t, "downloadedMap", { map: map.label }) })))
    : [], [current?.maps, locale, moduleDetails?.schema_json, supported, t]);
  return supported ? { status, choices, error: current?.error ?? null, refresh: () => {
    if (status !== "loading") setRevision((value) => value + 1);
  } } : undefined;
}
