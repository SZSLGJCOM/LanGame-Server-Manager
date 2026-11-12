import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";

export function isInteractiveWindowTarget(target: EventTarget | null) {
  return target instanceof Element && Boolean(target.closest(
    "button, input, select, textarea, option, a, label, [role='button'], [data-no-window-drag='true']"
  ));
}

/** Wait for a drag gesture so a click or double-click cannot restore the window. */
export function useWindowDrag(enabled: boolean) {
  const cancel = useRef<(() => void) | null>(null);
  useEffect(() => () => cancel.current?.(), []);

  return (event: ReactMouseEvent<HTMLElement>) => {
    if (!enabled || event.button !== 0 || event.detail === 2 || isInteractiveWindowTarget(event.target)) return;
    cancel.current?.();
    event.preventDefault();
    const start = { x: event.clientX, y: event.clientY };
    const anchor = { xRatio: start.x / window.innerWidth, y: start.y * window.devicePixelRatio };
    const stop = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
      window.removeEventListener("blur", stop);
      cancel.current = null;
    };
    const move = (next: MouseEvent) => {
      if ((next.buttons & 1) === 0) { stop(); return; }
      if (Math.hypot(next.clientX - start.x, next.clientY - start.y) < 4) return;
      stop();
      void invoke<void>("start_desktop_window_drag", { anchor }).catch((error) => {
        console.error("Window drag failed", error);
      });
    };
    cancel.current = stop;
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
    window.addEventListener("blur", stop);
  };
}
