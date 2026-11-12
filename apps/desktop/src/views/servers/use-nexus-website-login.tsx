import { Channel, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { isNexusWebsiteAuthState, NexusApiError, readNexusWebsiteAuthState, requestNexusCommand, type NexusWebsiteAuthState } from "../../nexus-mods-api";
import { visibleEmbeddedBrowserBounds } from "./embedded-browser-bounds";
import { nexusText } from "./nexus-mods-messages";

type LoginRequest = {
  sessionId: string | null; cancelled: boolean; scope: number; manageAccount: boolean;
  opened: Promise<void>; completeOpening: () => void;
  closing?: Promise<void>; failure?: unknown;
};

function LoginPage({ request, close, fail }: {
  request: LoginRequest;
  close: () => Promise<void>;
  fail: (request: LoginRequest, error: unknown) => Promise<void>;
}) {
  const { locale } = useI18n();
  const slot = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let disposed = false, frame = 0, updating = false, lastBounds = "null";
    const track = () => {
      if (disposed || request.cancelled) return;
      if (!updating) {
        const bounds = slot.current ? visibleEmbeddedBrowserBounds(slot.current) : null, key = JSON.stringify(bounds);
        if (key !== lastBounds) {
          lastBounds = key;
          updating = true;
          void requestNexusCommand("update_nexus_login_host", { sessionId: request.sessionId, bounds })
            .catch((error: unknown) => { if (!disposed) return fail(request, error); })
            .finally(() => { updating = false; });
        }
      }
      frame = requestAnimationFrame(track);
    };
    track();
    return () => { disposed = true; cancelAnimationFrame(frame); };
  }, [request, fail]);
  return <div className="mw-reference-browser" data-nexus-browser="login">
    <div className="mw-reference-browser-toolbar">
      <span role="status">{nexusText(locale, request.manageAccount ? "websiteAccountHelp" : "websiteLoginHelp")}</span>
      <button type="button" className="mw-ghost-btn" onClick={() => void close()}>{nexusText(locale, "websiteLoginClose")}</button>
    </div>
    <div className="mw-reference-browser-page" ref={slot} aria-label={nexusText(locale, "websiteLoginPage")} />
  </div>;
}

// Authentication comes only from native observations of official website DOM.
// Reading that cache never opens a page; only the explicit action reserves one.
export function useNexusWebsiteLogin(instanceId: string, scope: string, options: { enabled: boolean; downloading: boolean }) {
  const current = useRef<LoginRequest | null>(null);
  const scopeVersion = useRef(0), authVersion = useRef(0);
  const auth = useRef<NexusWebsiteAuthState>("unknown");
  const [authState, setAuthState] = useState<NexusWebsiteAuthState>("unknown");
  const [state, setState] = useState<"idle" | "opening" | "ready" | "closing">("idle");
  const [error, setError] = useState<unknown>(null);
  const finish = useCallback((request: LoginRequest, failure?: unknown): Promise<void> => {
    request.failure ??= failure;
    if (request.closing) return request.closing;
    request.cancelled = true;
    if (current.current === request) setState("closing");
    request.closing = (async () => {
      // Closing also awaits an in-flight reservation before releasing ownership.
      await request.opened;
      try {
        if (request.sessionId) await requestNexusCommand("close_nexus_login_host", { sessionId: request.sessionId });
      } catch (reason) { request.failure ??= reason; }
      if (current.current === request) {
        current.current = null;
        setState("idle");
        if (request.failure && request.scope === scopeVersion.current) setError(request.failure);
      }
    })();
    return request.closing;
  }, []);
  const close = useCallback(() => current.current ? finish(current.current) : Promise.resolve(), [finish]);
  useLayoutEffect(() => {
    scopeVersion.current++;
    setError(null);
    return () => { scopeVersion.current++; authVersion.current++; void close(); };
  }, [instanceId, scope, close]);
  useEffect(() => {
    if (!options.enabled || !isTauri() || options.downloading || state !== "idle") return;
    const version = ++authVersion.current, currentScope = scopeVersion.current;
    const update = (value: NexusWebsiteAuthState) => {
      if (authVersion.current !== version || scopeVersion.current !== currentScope) return;
      auth.current = value; setAuthState(value);
    };
    // A missing observation or failed cache read cannot prove sign-in or sign-out.
    void readNexusWebsiteAuthState().then(update, () => update("unknown"));
  }, [instanceId, scope, options.enabled, options.downloading, state]);
  const open = useCallback(async () => {
    if (current.current) return;
    if (!isTauri()) { setError(new NexusApiError("nexus_local_desktop_required")); return; }
    let completeOpening!: () => void;
    const opened = new Promise<void>((resolve) => { completeOpening = resolve; });
    const returnAfterSignIn = auth.current !== "signed_in";
    const request: LoginRequest = { sessionId: null, cancelled: false, scope: scopeVersion.current,
      manageAccount: !returnAfterSignIn, opened, completeOpening };
    current.current = request;
    authVersion.current++;
    setError(null); setState("opening");
    const onAuthChange = new Channel<string>((value) => {
      if (current.current !== request || request.cancelled || request.scope !== scopeVersion.current || !isNexusWebsiteAuthState(value)) return;
      authVersion.current++;
      auth.current = value; setAuthState(value);
      if (value === "signed_in" && returnAfterSignIn) void finish(request);
    });
    let failure: unknown;
    try {
      const sessionId = await requestNexusCommand("open_nexus_login_host", { bounds: null, onAuthChange });
      if (typeof sessionId !== "string" || !sessionId) throw new NexusApiError("nexus_invalid_response");
      request.sessionId = sessionId;
    } catch (reason) { if (!request.cancelled) failure = reason; }
    finally { request.completeOpening(); }
    if (failure || request.cancelled) await finish(request, failure);
    else setState("ready");
  }, [finish]);
  return { open, close, authState, pending: state !== "idle", opening: state === "opening", closing: state === "closing", error,
    view: state === "ready" && current.current ? <LoginPage request={current.current} close={close} fail={finish} /> : null };
}
