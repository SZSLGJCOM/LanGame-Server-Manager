import { useEffect, useRef } from "react";
import {
  createLibraryGamepadState,
  stepLibraryGamepad,
  type LibraryNavigationAction
} from "./library-navigation-model";

export interface UseLibraryGamepadNavigationOptions {
  enabled?: boolean;
  onAction: (action: LibraryNavigationAction) => void;
}

export function useLibraryGamepadNavigation({ enabled = true, onAction }: UseLibraryGamepadNavigationOptions) {
  const onActionRef = useRef(onAction);
  onActionRef.current = onAction;

  useEffect(() => {
    if (!enabled || typeof window === "undefined" || typeof navigator.getGamepads !== "function") {
      return;
    }

    let frameId: number | null = null;
    let activeGamepadIndex: number | null = null;
    let gamepadState = createLibraryGamepadState();

    function resetInput() {
      activeGamepadIndex = null;
      gamepadState = createLibraryGamepadState();
    }

    function readGamepad() {
      const gamepads = navigator.getGamepads();
      if (activeGamepadIndex !== null) {
        const active = gamepads[activeGamepadIndex];
        if (active?.connected) {
          return active;
        }
      }
      return Array.from(gamepads).find((gamepad): gamepad is Gamepad => Boolean(gamepad?.connected)) ?? null;
    }

    function poll(now: number) {
      if (document.visibilityState === "visible" && document.hasFocus()) {
        const gamepad = readGamepad();
        if (!gamepad) {
          resetInput();
        } else {
          if (activeGamepadIndex !== gamepad.index) {
            gamepadState = createLibraryGamepadState();
            activeGamepadIndex = gamepad.index;
          }
          const result = stepLibraryGamepad(gamepadState, gamepad, now);
          gamepadState = result.state;
          result.actions.forEach((action) => onActionRef.current(action));
        }
      }
      frameId = window.requestAnimationFrame(poll);
    }

    function handleVisibilityChange() {
      if (document.visibilityState !== "visible") {
        resetInput();
      }
    }

    function handleGamepadDisconnected(event: GamepadEvent) {
      if (activeGamepadIndex === event.gamepad.index) {
        resetInput();
      }
    }

    window.addEventListener("blur", resetInput);
    window.addEventListener("gamepaddisconnected", handleGamepadDisconnected);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    frameId = window.requestAnimationFrame(poll);

    return () => {
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }
      window.removeEventListener("blur", resetInput);
      window.removeEventListener("gamepaddisconnected", handleGamepadDisconnected);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [enabled]);
}
