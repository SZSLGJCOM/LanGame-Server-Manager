import { useCallback, useEffect, useRef, useState } from "react";
import { useInstanceSettingsSaveCoordinator } from "./InstanceSettingsSaveContext";

/** Native API drafts finish on navigation and before stop while the server is reachable. */
export function useSatisfactoryNativeAutosave<T>(options: {
  instanceId: string; signature: string; getDraft(): T | null; canSave(draft: T): boolean;
  save(draft: T): Promise<boolean>; onSaved(draft: T): void;
}) {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const latest = useRef(options);
  latest.current = options;
  const owner = useRef({});
  const running = useRef<Promise<void> | null>(null);
  const failure = useRef(false);
  const [failed, setFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const flush = useCallback((): Promise<void> => {
    if (running.current) return running.current;
    if (!latest.current.getDraft()) return Promise.resolve();
    if (failure.current) return Promise.reject(new Error("Native settings need review before retrying."));
    const completion = Promise.resolve().then(async () => {
      setSaving(true);
      for (;;) {
        const draft = latest.current.getDraft();
        if (!draft) return;
        if (!latest.current.canSave(draft)) throw new Error("The native settings draft cannot be saved yet.");
        if (!await latest.current.save(draft)) {
          failure.current = true; setFailed(true);
          throw new Error("Native settings were not confirmed. Refresh server state before retrying.");
        }
        latest.current.onSaved(draft);
      }
    }).finally(() => { running.current = null; setSaving(false); });
    running.current = completion;
    return completion;
  }, []);
  useEffect(() => {
    const registration = coordinator.register(options.instanceId, flush, owner.current, { beforeStop: true });
    return () => { registration.detach(flush()); };
  }, [coordinator, flush, options.instanceId]);
  useEffect(() => {
    if (!latest.current.getDraft()) { failure.current = false; setFailed(false); return; }
    if (failure.current || running.current) return;
    const draft = latest.current.getDraft();
    if (!draft || !latest.current.canSave(draft)) return;
    const timer = setTimeout(() => { void flush().catch(() => { setFailed(true); }); }, 800);
    return () => clearTimeout(timer);
  }, [flush, options.signature]);
  const retry = () => { failure.current = false; setFailed(false); void flush().catch(() => { setFailed(true); }); };
  return { failed, saving, retry };
}
