import { useEffect, useState } from "react";
import type { Window } from "@tauri-apps/api/window";

/** Keep the title-bar action in sync with native maximize, restore and snap. */
export function useWindowMaximized(window: Window | null): boolean {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!window) return;
    let disposed = false;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const current = ++revision;
      try {
        const value = await window.isMaximized();
        if (!disposed && current === revision) setMaximized(value);
      } catch (error) {
        if (!disposed) console.warn("Could not read native window state.", error);
      }
    };
    void window.onResized(() => { void refresh(); }).then((stop) => {
      if (disposed) stop();
      else { unlisten = stop; void refresh(); }
    }).catch((error) => {
      if (!disposed) console.warn("Could not observe native window state.", error);
    });
    return () => { disposed = true; unlisten?.(); };
  }, [window]);
  return maximized;
}
