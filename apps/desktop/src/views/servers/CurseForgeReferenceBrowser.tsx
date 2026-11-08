import { Channel, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { resolveManualModReferences } from "../../api";
import { invokeOrMock } from "../../api-transport";
import { useI18n } from "../../i18n";
import type { ManualModReferenceResolveResult } from "../../types";

type Bounds = { x: number; y: number; width: number; height: number };
type Request = {
  instanceId: string;
  references: string[];
  settled: boolean;
  resolve: (result: ManualModReferenceResolveResult) => void;
  reject: (error: unknown) => void;
};
const cancelled = () => new Error(JSON.stringify({ code: "curseforge_reference_failed", reason: "cancelled" }));

// CSS pixels also change with WebView zoom. Send physical client coordinates
// so the native child stays inside this exact DOM slot at any DPI/zoom level.
function visibleBounds(element: HTMLElement): Bounds | null {
  const rect = element.getBoundingClientRect();
  if (document.hidden || rect.width < 2 || rect.height < 2 || rect.left < 0 || rect.top < 0
    || rect.right > innerWidth || rect.bottom > innerHeight) return null;
  for (const dialog of document.querySelectorAll('[role="dialog"][aria-modal="true"], [role="alertdialog"], dialog[open]')) {
    if (dialog.getClientRects().length) return null;
  }
  for (const [x, y] of [[rect.left + 1, rect.top + 1], [rect.right - 1, rect.top + 1],
    [rect.left + 1, rect.bottom - 1], [rect.right - 1, rect.bottom - 1],
    [rect.left + rect.width / 2, rect.top + rect.height / 2]]) {
    if (!element.contains(document.elementFromPoint(x, y))) return null;
  }
  const x = Math.round(rect.left * devicePixelRatio), y = Math.round(rect.top * devicePixelRatio);
  return { x, y, width: Math.round(rect.right * devicePixelRatio) - x, height: Math.round(rect.bottom * devicePixelRatio) - y };
}

function EmbeddedBrowser({ request, finish }: {
  request: Request;
  finish: (request: Request, result?: ManualModReferenceResolveResult, error?: unknown) => void;
}) {
  const { t } = useI18n();
  const slot = useRef<HTMLDivElement>(null);
  const [verificationRequired, setVerificationRequired] = useState(false);
  const verification = useRef(false);
  useEffect(() => {
    // A scope change can move the JSX into another pane in the same commit
    // that cancels it. That remount must not reserve another native host.
    if (request.settled) return;
    let disposed = false, stopped = false, frame = 0, updating = false;
    let sessionId: string | undefined, lastBounds = "null";
    let closing: Promise<unknown> | undefined;
    const close = () => sessionId ? closing ??= invokeOrMock("close_curseforge_browser_host", { sessionId }) : Promise.resolve();
    const track = () => {
      if (disposed || stopped) return;
      if (sessionId && !updating) {
        const bounds = verification.current && slot.current ? visibleBounds(slot.current) : null, key = JSON.stringify(bounds);
        if (key !== lastBounds) {
          lastBounds = key;
          updating = true;
          void invokeOrMock("update_curseforge_browser_host", { sessionId, bounds })
            .catch((error) => { if (!disposed) finish(request, undefined, error); })
            .finally(() => { updating = false; });
        }
      }
      frame = requestAnimationFrame(track);
    };
    void (async () => {
      // StrictMode replays mount effects in development. Start native work only
      // after that commit's cleanup has had a chance to discard the first run.
      await Promise.resolve();
      if (disposed || request.settled) return;
      let result: ManualModReferenceResolveResult | undefined, failure: unknown;
      try {
        const onProgress = new Channel<"loading" | "verification_required">((progress) => {
          if (disposed || stopped || request.settled) return;
          verification.current = progress === "verification_required";
          // A new page may request the same rectangle as the previous one.
          // Native code clears its bounds between pages, so re-send them.
          lastBounds = "";
          setVerificationRequired(verification.current);
        });
        sessionId = await invokeOrMock<string>("open_curseforge_browser_host", { bounds: null, onProgress });
        if (disposed) return;
        // Re-measure after opening: the parent may have moved during the IPC.
        track();
        result = await resolveManualModReferences(request.instanceId, request.references, "arksurvivalascended", sessionId);
      } catch (error) {
        failure = error;
      } finally {
        stopped = true;
        cancelAnimationFrame(frame);
        try { await close(); } catch (error) { failure ??= error; }
        if (!disposed) finish(request, result, failure);
      }
    })();
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      // If opening is still pending, its finally block closes the returned ID.
      void close().catch((error) => console.error("Close CurseForge recognition area", error));
    };
  }, [request, finish]);
  if (!verificationRequired) return null;
  return <div className="mw-reference-browser" data-curseforge-browser="embedded">
    <div className="mw-reference-browser-toolbar">
      <span role="status">{t("servers.mods.curseforgeBrowser.verification")}</span>
      <button type="button" className="mw-ghost-btn" onClick={() => finish(request, undefined, cancelled())}>
        {t("common.cancel", undefined, "Cancel")}
      </button>
    </div>
    <div className="mw-reference-browser-page" ref={slot} aria-label={t("servers.mods.curseforgeBrowser.page")} />
  </div>;
}

export function useCurseForgeReferenceBrowser(instanceId: string, scope: string) {
  const [request, setRequest] = useState<Request | null>(null);
  const current = useRef<Request | null>(null);
  const finish = useCallback((item: Request, result?: ManualModReferenceResolveResult, error?: unknown) => {
    if (item.settled) return;
    item.settled = true;
    if (current.current === item) { current.current = null; setRequest(null); }
    if (error !== undefined || !result) item.reject(error ?? cancelled());
    else item.resolve(result);
  }, []);
  useLayoutEffect(() => () => {
    if (current.current) finish(current.current, undefined, cancelled());
  }, [instanceId, scope, finish]);
  const resolve = useCallback((references: string[]) => {
    if (!isTauri() || !references.some((reference) => /^https?:\/\/(?:[a-z0-9-]+\.)*curseforge\.com\/ark-survival-ascended\/mods\//i.test(reference))) {
      return resolveManualModReferences(instanceId, references, "arksurvivalascended");
    }
    return new Promise<ManualModReferenceResolveResult>((resolve, reject) => {
      if (current.current) { reject(new Error(JSON.stringify({ code: "curseforge_reference_failed", reason: "busy" }))); return; }
      const item = { instanceId, references, resolve, reject, settled: false };
      current.current = item;
      setRequest(item);
    });
  }, [instanceId]);
  const cancel = useCallback(() => {
    if (current.current) finish(current.current, undefined, cancelled());
  }, [finish]);
  return { resolve, cancel, pending: request !== null, view: request ? <EmbeddedBrowser request={request} finish={finish} /> : null };
}
