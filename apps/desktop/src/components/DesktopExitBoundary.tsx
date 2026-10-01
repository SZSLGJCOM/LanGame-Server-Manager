import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { desktopExitLifecycle, type DesktopExitLifecycle } from "../desktop-exit-lifecycle";
import { useI18n, selectLocaleText } from "../i18n";
import "../styles/desktop-exit.css";

export function DesktopExitBoundary({ children, lifecycle = desktopExitLifecycle }: {
  children: ReactNode;
  lifecycle?: DesktopExitLifecycle;
}) {
  const status = useSyncExternalStore(lifecycle.subscribe, lifecycle.getSnapshot);
  const [ready, setReady] = useState(!lifecycle.enabled || status.requested);
  const heading = useRef<HTMLHeadingElement>(null);
  const { locale, t } = useI18n();

  useEffect(() => {
    if (!lifecycle.enabled || lifecycle.getSnapshot().requested) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    async function connect() {
      try {
        const dispose = await lifecycle.listen();
        if (!active) { dispose(); return; }
        unlisten = dispose;
      } catch (error) {
        console.error("Desktop exit event subscription failed.", error);
      }
      try {
        await lifecycle.refresh();
      } catch (error) {
        // A status-read failure is not proof of shutdown and must not lock the UI.
        console.error("Desktop exit status could not be read.", error);
      } finally {
        if (active) setReady(true);
      }
    }
    void connect();
    return () => { active = false; unlisten?.(); };
  }, [lifecycle]);

  useEffect(() => { if (status.requested) heading.current?.focus(); }, [status.requested]);

  if (!status.requested && ready) return children;

  return <main className="desktop-exit-page" role="status" aria-live="polite">
    <div className="desktop-exit-content">
      <h1 ref={heading} tabIndex={-1}>{status.requested
        ? selectLocaleText(locale, "正在退出", "Exiting")
        : t("storage.initialization.loadingTitle")}</h1>
      {status.requested && <p className="desktop-exit-detail">{selectLocaleText(locale,
        "正在交接服务器停止任务。", "Handing off server stop requests.")}</p>}
    </div>
  </main>;
}
