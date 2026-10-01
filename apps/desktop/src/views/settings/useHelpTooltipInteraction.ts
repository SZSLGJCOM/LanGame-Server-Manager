import {
  useCallback, useLayoutEffect, useRef, useState,
  type HTMLAttributes, type PointerEvent, type RefObject
} from "react";

const OPEN_DELAY_MS = 240;
const TRANSFER_DELAY_MS = 100;
const EXIT_DURATION_MS = 140;
const CLAIM_EVENT = "configuration-help-claim";
const ANCHOR_SELECTOR = "[data-configuration-help-anchor]";
type Phase = "pending" | "measuring" | "visible" | "leaving" | null;

/** Own input modality, pending work and dismissal for one help anchor and its portal. */
export function useHelpTooltipInteraction(
  enabled: boolean,
  anchor: RefObject<HTMLElement | null>,
  bubble: RefObject<HTMLElement | null>,
  canShow: () => boolean
) {
  const [phase, setPhase] = useState<Phase>(null);
  const currentPhase = useRef<Phase>(null);
  const pointerWithin = useRef(false);
  const pointerOverBubble = useRef(false);
  const keyboardFocus = useRef(false);
  const pointerFocus = useRef(false);
  const dismissed = useRef(false);
  const pointerPosition = useRef<{ x: number; y: number } | null>(null);
  const dismissedPosition = useRef<{ x: number; y: number } | null>(null);
  const openTimer = useRef<number | null>(null);
  const transferTimer = useRef<number | null>(null);
  const exitTimer = useRef<number | null>(null);
  const eligibility = useRef(canShow);
  eligibility.current = canShow;

  const transition = useCallback((next: Phase) => {
    currentPhase.current = next;
    setPhase(next);
  }, []);
  const clearTimer = (timer: RefObject<number | null>) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };
  const clearTimers = useCallback(() => {
    clearTimer(openTimer);
    clearTimer(transferTimer);
    clearTimer(exitTimer);
  }, []);
  const hide = useCallback((immediate = false) => {
    clearTimers();
    if (immediate || currentPhase.current === "pending" || currentPhase.current === null) {
      transition(null);
    } else {
      transition("leaving");
      exitTimer.current = window.setTimeout(() => {
        exitTimer.current = null;
        transition(null);
      }, EXIT_DURATION_MS);
    }
  }, [clearTimers, transition]);
  const dismiss = useCallback(() => {
    dismissed.current = true;
    dismissedPosition.current = pointerPosition.current;
    pointerOverBubble.current = false;
    keyboardFocus.current = false;
    hide(true);
  }, [hide]);
  const owns = useCallback((target: EventTarget | null) => target instanceof Element
    && target.closest(ANCHOR_SELECTOR) === anchor.current, [anchor]);
  const overBubble = useCallback((target: EventTarget | null) => target instanceof Node
    && Boolean(bubble.current?.contains(target)), [bubble]);

  const show = useCallback((delayed: boolean) => {
    if (!enabled || dismissed.current || !eligibility.current()) return;
    clearTimer(transferTimer);
    clearTimer(exitTimer);
    if (currentPhase.current === "visible" || currentPhase.current === "measuring") return;
    if (currentPhase.current === "leaving") {
      transition("visible");
      return;
    }
    const open = () => {
      openTimer.current = null;
      if (!dismissed.current && (pointerWithin.current || keyboardFocus.current) && eligibility.current()) {
        transition("measuring");
      } else transition(null);
    };
    if (!delayed) {
      clearTimer(openTimer);
      open();
    } else if (openTimer.current === null) {
      transition("pending");
      openTimer.current = window.setTimeout(open, OPEN_DELAY_MS);
    }
  }, [enabled, transition]);
  const leave = useCallback(() => {
    if (keyboardFocus.current || pointerWithin.current || pointerOverBubble.current) return;
    clearTimer(openTimer);
    if (currentPhase.current === "pending") {
      hide(true);
      return;
    }
    if (currentPhase.current === null || currentPhase.current === "leaving" || transferTimer.current !== null) return;
    // Only a visible bubble needs a grace period for crossing its eight-pixel gap.
    transferTimer.current = window.setTimeout(() => {
      transferTimer.current = null;
      if (!keyboardFocus.current && !pointerWithin.current && !pointerOverBubble.current) hide();
    }, TRANSFER_DELAY_MS);
  }, [hide]);

  const active = phase !== null;
  useLayoutEffect(() => {
    if (!active) return;
    const claim = (event: Event) => {
      if ((event as CustomEvent<HTMLElement>).detail !== anchor.current) dismiss();
    };
    // Only active/pending help listens. A new owner immediately retires the old portal.
    document.addEventListener(CLAIM_EVENT, claim);
    document.dispatchEvent(new CustomEvent(CLAIM_EVENT, { detail: anchor.current }));
    const keydown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && !event.isComposing) dismiss();
      if (event.key === "Tab") pointerFocus.current = false;
    };
    const visibility = () => { if (document.hidden) dismiss(); };
    const pointerout = (event: globalThis.PointerEvent) => { if (!event.relatedTarget) dismiss(); };
    const pointermove = (event: globalThis.PointerEvent) => {
      if (event.pointerType === "touch") return;
      if (event.isTrusted) pointerPosition.current = { x: event.clientX, y: event.clientY };
      // Hit testing also handles disabled descendants and captured pointer moves.
      const target = document.elementFromPoint(event.clientX, event.clientY);
      pointerWithin.current = owns(target);
      pointerOverBubble.current = overBubble(target);
      if (pointerWithin.current || pointerOverBubble.current) clearTimer(transferTimer);
      else leave();
    };
    const pointerdown = (event: globalThis.PointerEvent) => {
      keyboardFocus.current = false;
      if (!owns(event.target) && !overBubble(event.target)) dismiss();
    };
    const scroll = (event: Event) => {
      // Scrolling the help itself is reading; scrolling its surroundings ends hover.
      if (!overBubble(event.target) && !keyboardFocus.current) dismiss();
    };
    window.addEventListener("blur", dismiss);
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("keydown", keydown, true);
    document.addEventListener("pointerout", pointerout);
    document.addEventListener("pointermove", pointermove, true);
    document.addEventListener("pointerdown", pointerdown, true);
    document.addEventListener("pointercancel", dismiss, true);
    window.addEventListener("scroll", scroll, true);
    return () => {
      document.removeEventListener(CLAIM_EVENT, claim);
      window.removeEventListener("blur", dismiss);
      document.removeEventListener("visibilitychange", visibility);
      window.removeEventListener("keydown", keydown, true);
      document.removeEventListener("pointerout", pointerout);
      document.removeEventListener("pointermove", pointermove, true);
      document.removeEventListener("pointerdown", pointerdown, true);
      document.removeEventListener("pointercancel", dismiss, true);
      window.removeEventListener("scroll", scroll, true);
    };
  }, [active, anchor, dismiss, leave, overBubble, owns]);
  useLayoutEffect(() => { if (!enabled) dismiss(); }, [dismiss, enabled]);
  useLayoutEffect(() => clearTimers, [clearTimers]);

  const enter = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === "touch" || overBubble(event.target)) return;
    if (!owns(event.target)) {
      pointerWithin.current = false;
      keyboardFocus.current = false;
      hide(true);
      return;
    }
    if (pointerWithin.current) return;
    pointerWithin.current = true;
    const last = dismissedPosition.current;
    // Removing an overlapping portal can emit pointerover without any mouse movement.
    // It must not undo Escape; leaving this region or entering at a new point rearms help.
    if (event.nativeEvent.isTrusted && dismissed.current && last
      && event.clientX === last.x && event.clientY === last.y) return;
    if (event.nativeEvent.isTrusted) pointerPosition.current = { x: event.clientX, y: event.clientY };
    dismissed.current = false;
    show(true);
  };
  const interactionProps: HTMLAttributes<HTMLElement> = enabled ? {
    onPointerEnter: enter,
    onPointerOver: enter,
    onPointerLeave: () => {
      pointerWithin.current = false;
      dismissedPosition.current = null;
      leave();
    },
    onPointerDownCapture: (event) => {
      if (!owns(event.target)) return;
      // Clicking a text input also produces :focus-visible; that is not keyboard intent.
      pointerFocus.current = true;
      keyboardFocus.current = false;
      if (event.pointerType === "touch") dismiss();
    },
    onFocusCapture: (event) => {
      if (!owns(event.target)) {
        keyboardFocus.current = false;
        hide(true);
        return;
      }
      keyboardFocus.current = !pointerFocus.current;
      if (keyboardFocus.current) {
        dismissed.current = false;
        show(false);
      }
    },
    onBlurCapture: (event) => {
      if (owns(event.relatedTarget)) return;
      keyboardFocus.current = false;
      pointerFocus.current = false;
      leave();
    },
    onKeyDownCapture: (event) => {
      if (event.key === "Tab") pointerFocus.current = false;
      if (event.key === "Escape" && !event.nativeEvent.isComposing) dismiss();
    }
  } : {};
  const bubbleProps: HTMLAttributes<HTMLElement> = {
    onPointerEnter: (event) => {
      if (event.pointerType === "touch" || dismissed.current) return;
      pointerOverBubble.current = true;
      clearTimer(transferTimer);
      clearTimer(exitTimer);
      if (currentPhase.current === "leaving") transition("visible");
    },
    onPointerLeave: () => {
      pointerOverBubble.current = false;
      leave();
    }
  };
  return { phase, interactionProps, bubbleProps, dismiss,
    markVisible: useCallback(() => {
      if (currentPhase.current === "measuring") transition("visible");
    }, [transition]) };
}
