import { useEffect, useRef, useState } from "react";
import { manageManualMod } from "../../api";
import { formatDesktopError } from "../../desktop-error-message";
import { useI18n } from "../../i18n";
import type { ManagedModAction, ManagedModActionResult, ManualModInventoryItem } from "../../types";

interface Options {
  instanceId: string;
  runOperation: (operation: () => Promise<void>) => Promise<void>;
  onResult: (result: ManagedModActionResult) => void;
}

/** File operations share the workbench's instance mutation lock with installs. */
export function useManagedManualMods(options: Options) {
  const { t } = useI18n();
  const [review, setReview] = useState<ManualModInventoryItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ action: ManagedModAction; value: ManagedModActionResult } | null>(null);
  const currentInstance = useRef(options.instanceId);
  const scope = useRef(0);
  currentInstance.current = options.instanceId;

  useEffect(() => {
    scope.current += 1;
    setReview(null);
    setError(null);
    setResult(null);
    return () => { scope.current += 1; };
  }, [options.instanceId]);

  async function change(item: ManualModInventoryItem, action: ManagedModAction) {
    const instanceId = options.instanceId;
    const token = scope.current;
    const isCurrent = () => scope.current === token && currentInstance.current === instanceId;
    if (!isCurrent()) return;
    setError(null);
    setResult(null);
    try {
      await options.runOperation(async () => {
        if (!isCurrent()) return;
        const value = await manageManualMod(instanceId, item.management?.package_id ?? item.name, action);
        // A completed filesystem action belongs to its original instance even
        // when the user navigates elsewhere while it is running.
        if (!isCurrent()) return;
        options.onResult(value);
        setResult({ action, value });
        if (action === "delete") setReview(null);
      });
    } catch (cause) {
      if (!isCurrent()) return;
      setError(t("servers.mods.managed.failed", { message: formatDesktopError(t, cause) }, "Mod change failed: {message}"));
    }
  }

  return {
    review, error, result,
    change: (item: ManualModInventoryItem, action: ManagedModAction) => { void change(item, action); },
    requestDelete: (item: ManualModInventoryItem) => { setError(null); setReview(item); },
    close: () => { setReview(null); setError(null); }
  };
}
