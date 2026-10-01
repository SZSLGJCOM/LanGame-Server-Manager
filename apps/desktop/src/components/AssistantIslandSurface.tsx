import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { motion, useReducedMotion } from "motion/react";
import { assistantPanelStyle } from "./assistant-panel-position";

interface AssistantIslandSurfaceProps {
  open: boolean;
  anchorRect: DOMRect | null;
  onExited: () => void;
  onEntered: () => void;
  onClose: () => void;
  closeLabel: string;
  panelTitle: string;
  originContent: ReactNode;
  surfaceRef: RefObject<HTMLDivElement | null>;
  children: ReactNode;
}

export function AssistantIslandSurface({
  open, anchorRect, onExited, onEntered, onClose, closeLabel, panelTitle, originContent, surfaceRef, children
}: AssistantIslandSurfaceProps) {
  const reducedMotion = useReducedMotion();
  const [settled, setSettled] = useState(false);
  const toggleRef = useRef<HTMLButtonElement | null>(null);
  const focusPending = useRef(open);
  const [viewport, setViewport] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));

  useLayoutEffect(() => {
    setSettled(false);
    focusPending.current = open;
    if (open) toggleRef.current?.focus({ preventScroll: true });
  }, [open]);

  useLayoutEffect(() => {
    if (open && settled && focusPending.current) {
      focusPending.current = false;
      onEntered();
    }
  }, [open, settled, onEntered]);

  useEffect(() => {
    const resize = () => setViewport({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);

  const style = assistantPanelStyle(anchorRect, viewport.width);
  const width = style ? parseFloat(style["--assistant-panel-width"]) : Math.min(480, viewport.width - 40);
  const left = style ? parseFloat(style["--assistant-panel-left"]) : (viewport.width - width) / 2;
  const top = style ? parseFloat(style["--assistant-panel-top"]) : 96;
  const height = Math.max(0, Math.min(560, viewport.height - top - 20));
  const expanded = { left, top, width, height, borderRadius: 24 };
  const collapsed = anchorRect ? {
    left: anchorRect.left, top: anchorRect.top, width: anchorRect.width,
    height: anchorRect.height, borderRadius: anchorRect.height / 2
  } : expanded;

  return (
    <motion.div
      ref={surfaceRef}
      className="assistant-island-surface"
      role="dialog"
      aria-modal="true"
      aria-label={panelTitle}
      data-state={open ? settled ? "open" : "opening" : "closing"}
      data-no-window-drag="true"
      inert={!open}
      aria-hidden={!open || undefined}
      initial={reducedMotion ? expanded : collapsed}
      animate={open ? expanded : collapsed}
      transition={reducedMotion ? { duration: 0 } : {
        type: "spring", stiffness: open ? 380 : 460, damping: open ? 38 : 43, mass: 1,
        restDelta: 0.25, restSpeed: 0.5
      }}
      onAnimationComplete={() => {
        if (!open) { onExited(); return; }
        setSettled(true);
      }}
    >
      {/* Keep text at its final dimensions: only the shell changes shape. */}
      <motion.div
        className="assistant-island-content"
        inert={!open || !settled}
        style={{ width: width - 2, height: height - 2 }}
        initial={reducedMotion ? false : { opacity: 0, y: -8, filter: "blur(3px)" }}
        animate={open ? { opacity: 1, y: 0, filter: "blur(0px)" } : { opacity: 0, y: -6, filter: "blur(3px)" }}
        transition={reducedMotion ? { duration: 0 } : {
          duration: open ? 0.24 : 0.1, delay: open ? 0.1 : 0, ease: [0.22, 1, 0.36, 1]
        }}
      >
        {children}
      </motion.div>
      <motion.button
        ref={toggleRef}
        type="button"
        className="assistant-island-toggle"
        aria-label={closeLabel}
        title={closeLabel}
        aria-expanded={open}
        onClick={onClose}
        initial={{ width: anchorRect?.width ?? 126, top: -1 }}
        animate={{ width: open ? 60 : anchorRect?.width ?? 126, top: open ? 6 : -1 }}
        transition={reducedMotion ? { duration: 0 } : { type: "spring", stiffness: 380, damping: 38 }}
      >
        {originContent}
      </motion.button>
    </motion.div>
  );
}
