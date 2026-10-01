import { useCallback, useRef, useState, type RefObject } from "react";
import { formatDesktopError } from "../../desktop-error-message";
import type { TranslateFn } from "../../i18n";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import type { SettingsObject } from "../settings/settings-schema";
import { DST_RAW_MOD_WARNING, hasDstRawModOverrides } from "./mod-workbench-dst-policy";

type ModMutationKind = "install" | "configuration" | "enablement" | "staging";

export function useModWorkbenchMutations(
  instanceId: string,
  moduleId: string,
  blocked: RefObject<boolean>,
  settings: RefObject<SettingsObject>,
  t: TranslateFn
) {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [mutationBusy, setMutationBusy] = useState(false);
  const [mutationError, setMutationError] = useState<string | null>(null);
  const mutationToken = useRef<symbol | null>(null);

  const runModMutation = useCallback(async (kind: ModMutationKind, operation: () => Promise<void>) => {
    if (blocked.current) {
      throw new Error(t("servers.mods.stopBeforeChanges", undefined, "Stop the instance before changing Mods."));
    }
    if (mutationToken.current) {
      throw new Error(t("servers.mods.mutationBusy", undefined, "Another Mod change is still in progress."));
    }
    const token = Symbol(kind);
    mutationToken.current = token;
    setMutationBusy(true);
    setMutationError(null);
    try {
      await coordinator.runOperation(instanceId, async () => {
        if (moduleId === "dontstarve" && kind === "enablement" &&
          hasDstRawModOverrides(settings.current)) {
          throw new Error(t("dst.settings.modStatus.rawOverrideWarning", undefined, DST_RAW_MOD_WARNING));
        }
        await operation();
      }, `mod-workbench:${kind}`);
    } finally {
      if (mutationToken.current === token) {
        mutationToken.current = null;
        setMutationBusy(false);
      }
    }
  }, [blocked, coordinator, instanceId, moduleId, settings, t]);

  const launchModMutation = useCallback((kind: ModMutationKind, operation: () => Promise<void>) => {
    void runModMutation(kind, operation).catch((error) => {
      setMutationError(t("servers.mods.mutationFailed", { message: formatDesktopError(t, error) }, "Mod change failed: {message}"));
    });
  }, [runModMutation, t]);

  return { mutationBusy, mutationError, setMutationError, runModMutation, launchModMutation };
}
