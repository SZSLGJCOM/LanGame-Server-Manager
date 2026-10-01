import { useEffect, useState } from "react";
import { readDontStarveModConfigurationSpecs } from "../../api";
import { useI18n } from "../../i18n";
import type { DstModConfigurationSpec } from "../../types";

interface ConfigurationReadState {
  requestKey: string;
  specs: DstModConfigurationSpec[];
  loading: boolean;
  error: string | null;
}

export function useDstModConfigurationSpec(
  instanceId: string,
  selectedModId: string | null | undefined,
  scanNonce = 0,
  enabled = true
) {
  const { locale, t } = useI18n();
  const modId = selectedModId?.trim() || null;
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<ConfigurationReadState>({
    requestKey: "", specs: [], loading: false, error: null
  });
  const requestKey = JSON.stringify([instanceId, modId, locale, scanNonce, attempt]);
  const missingResult = t(
    "dst.settings.modStatus.specMissingResult",
    undefined,
    "The configuration reader did not return a result for this Mod."
  );

  useEffect(() => {
    if (!enabled || !modId) return;
    let cancelled = false;
    setState({ requestKey, specs: [], loading: true, error: null });
    readDontStarveModConfigurationSpecs(instanceId, [modId], locale)
      .then((specs) => {
        if (cancelled) return;
        const spec = specs.find((item) => item.mod_id === modId);
        setState({
          requestKey,
          specs: spec ? [spec] : [],
          loading: false,
          error: spec ? null : missingResult
        });
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setState({
          requestKey, specs: [], loading: false,
          error: reason instanceof Error ? reason.message : String(reason)
        });
      });
    return () => { cancelled = true; };
  }, [enabled, instanceId, locale, missingResult, modId, requestKey]);

  // Selection changes must hide old fields before the next effect runs.
  const current = enabled && modId && state.requestKey === requestKey ? state : null;
  return {
    specs: current?.specs ?? [],
    loading: enabled && (current?.loading ?? Boolean(modId)),
    error: current?.error ?? null,
    retry: () => setAttempt((value) => value + 1)
  };
}
