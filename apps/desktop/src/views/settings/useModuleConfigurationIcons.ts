import { useEffect, useMemo, useRef, useState } from "react";
import { readModuleConfigurationIcons } from "../../api";
import type { ModuleDetails } from "../../types";

const EMPTY_ICONS: Readonly<Record<string, string>> = Object.freeze({});
const MAX_ICON_RETRIES = 2;
// Module IDs lack installation context, so remounts share only the same loaded descriptor's pending work.
const pendingIconRequests = new WeakMap<ModuleDetails, Promise<Readonly<Record<string, string>>>>();

interface IconRequestScope {
  moduleDetails: ModuleDetails | null;
  instanceId: string;
}

interface IconLoadState {
  scope: IconRequestScope | null;
  attempt: number;
  status: "idle" | "loading" | "ready" | "failed";
  icons: Readonly<Record<string, string>>;
  error: Error | null;
  missing: boolean;
}

function validateIconResponse(value: unknown): Readonly<Record<string, string>> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Module configuration icons must be an object.");
  }
  const entries = Object.entries(value);
  if (entries.some(([, icon]) => typeof icon !== "string" ||
    !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(icon))) {
    throw new Error("Module configuration icons must contain PNG data URLs.");
  }
  return Object.fromEntries(entries);
}

function readPendingModuleIcons(moduleDetails: ModuleDetails): Promise<Readonly<Record<string, string>>> {
  const pending = pendingIconRequests.get(moduleDetails);
  if (pending) return pending;
  const request = readModuleConfigurationIcons(moduleDetails.summary.id).then(validateIconResponse);
  pendingIconRequests.set(moduleDetails, request);
  const release = () => { pendingIconRequests.delete(moduleDetails); };
  void request.then(release, release);
  return request;
}

export function useModuleConfigurationIcons(moduleDetails: ModuleDetails | null, instanceId: string) {
  const scope = useMemo(() => ({ moduleDetails, instanceId }), [moduleDetails, instanceId]);
  const [request, setRequest] = useState({ scope, attempt: 0 });
  const [state, setState] = useState<IconLoadState>({
    scope: null, attempt: 0, status: "idle", icons: EMPTY_ICONS, error: null, missing: false
  });
  const requestRef = useRef<{
    scope: IconRequestScope;
    attempt: number;
    promise: Promise<Readonly<Record<string, string>>>;
  } | null>(null);
  const supported = moduleDetails?.summary.id === "dontstarve";
  const attempt = request.scope === scope ? request.attempt : 0;
  const current = state.scope === scope ? state : null;
  const loading = supported && (!current || current.attempt !== attempt || current.status === "loading");

  useEffect(() => {
    if (!supported || !moduleDetails) return;
    let cancelled = false;
    setState((previous) => ({
      scope, attempt, status: "loading", icons: EMPTY_ICONS,
      error: previous.scope === scope ? previous.error : null,
      missing: previous.scope === scope && previous.missing
    }));
    if (requestRef.current?.scope !== scope || requestRef.current.attempt !== attempt) {
      requestRef.current = {
        scope, attempt,
        promise: readPendingModuleIcons(moduleDetails)
      };
    }
    void requestRef.current.promise.then((icons) => {
      if (cancelled) return;
      setState({ scope, attempt, status: "ready", icons, error: null, missing: Object.keys(icons).length === 0 });
    }).catch((cause: unknown) => {
      if (cancelled) return;
      const error = cause instanceof Error ? cause : new Error(String(cause));
      setState({ scope, attempt, status: "failed", icons: EMPTY_ICONS, error, missing: false });
    });
    return () => { cancelled = true; };
  }, [attempt, moduleDetails, scope, supported]);

  return {
    icons: supported ? current?.icons ?? EMPTY_ICONS : EMPTY_ICONS,
    error: supported ? current?.error ?? null : null,
    missing: supported && current?.missing === true,
    loading,
    retryAvailable: supported && attempt < MAX_ICON_RETRIES,
    retry: () => {
      if (!loading && (current?.status === "failed" || current?.missing) && attempt < MAX_ICON_RETRIES) {
        setRequest({ scope, attempt: attempt + 1 });
      }
    }
  };
}
