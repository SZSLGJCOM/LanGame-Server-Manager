export interface LibraryFocusTarget {
  moduleId: string;
}

export type LibraryInputSource = "pointer" | "keyboard" | "gamepad";
export type LibraryDirection = "left" | "right" | "up" | "down";

export type LibraryNavigationAction =
  | { type: "move"; direction: LibraryDirection; source: LibraryInputSource }
  | { type: "page"; direction: "left" | "right"; source: LibraryInputSource }
  | { type: "confirm"; source: LibraryInputSource }
  | { type: "back"; source: LibraryInputSource }
  | { type: "context"; source: LibraryInputSource };

export interface LibraryKeyboardInput {
  key: string;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  repeat?: boolean;
}

export type LibraryGamepadButtonSnapshot = boolean | Pick<GamepadButton, "pressed" | "value"> | null | undefined;

export interface LibraryGamepadSnapshot {
  axes: readonly number[];
  buttons: readonly LibraryGamepadButtonSnapshot[];
  connected?: boolean;
}

interface LibraryGamepadButtonLatches {
  confirm: boolean;
  back: boolean;
  context: boolean;
  pageLeft: boolean;
  pageRight: boolean;
}

export interface LibraryGamepadState {
  axisDirection: LibraryDirection | null;
  navigationDirection: LibraryDirection | null;
  nextNavigationRepeatAt: number | null;
  buttons: LibraryGamepadButtonLatches;
}

export interface LibraryGamepadStepResult {
  state: LibraryGamepadState;
  actions: LibraryNavigationAction[];
}

export const LIBRARY_GAMEPAD_AXIS_PRESS_THRESHOLD = 0.62;
export const LIBRARY_GAMEPAD_AXIS_RELEASE_THRESHOLD = 0.42;
export const LIBRARY_GAMEPAD_REPEAT_DELAY_MS = 320;
export const LIBRARY_GAMEPAD_REPEAT_INTERVAL_MS = 100;
export const LIBRARY_CATALOG_POINTER_CONFIRM_DELAY_MS = 260;

const GAMEPAD_AXIS_SWITCH_MARGIN = 0.08;

const GAMEPAD_BUTTON = {
  confirm: 0,
  back: 1,
  context: 2,
  pageLeft: 4,
  pageRight: 5,
  up: 12,
  down: 13,
  left: 14,
  right: 15
} as const;

export function libraryCatalogOptionId(target: LibraryFocusTarget) {
  return `library-catalog-${encodeURIComponent(target.moduleId)}`;
}

export function canConfirmLibraryCatalogPointerSelection(
  active: boolean,
  activeSinceMs: number | null,
  nowMs: number
) {
  return active
    && activeSinceMs !== null
    && nowMs - activeSinceMs >= LIBRARY_CATALOG_POINTER_CONFIRM_DELAY_MS;
}

export function resolveLibraryKeyboardAction(input: LibraryKeyboardInput): LibraryNavigationAction | null {
  if (input.altKey && input.key === "ArrowLeft") {
    return input.repeat ? null : { type: "back", source: "keyboard" };
  }

  if (input.ctrlKey || input.metaKey || input.altKey) {
    return null;
  }

  if ((input.key === "ContextMenu" || (input.shiftKey && input.key === "F10")) && !input.repeat) {
    return { type: "context", source: "keyboard" };
  }

  const directionByKey: Partial<Record<string, LibraryDirection>> = {
    ArrowLeft: "left",
    ArrowRight: "right",
    ArrowUp: "up",
    ArrowDown: "down"
  };
  const direction = directionByKey[input.key];
  if (direction) {
    return { type: "move", direction, source: "keyboard" };
  }

  if (input.key === "PageUp") {
    return { type: "page", direction: "left", source: "keyboard" };
  }
  if (input.key === "PageDown") {
    return { type: "page", direction: "right", source: "keyboard" };
  }
  if ((input.key === "Enter" || input.key === " ") && !input.repeat) {
    return { type: "confirm", source: "keyboard" };
  }
  if (input.key === "Escape" && !input.repeat) {
    return { type: "back", source: "keyboard" };
  }
  return null;
}

export function resolveLibraryHorizontalFocusId(
  ids: readonly string[],
  currentId: string | null,
  direction: "left" | "right"
) {
  if (ids.length === 0) {
    return null;
  }

  const currentIndex = ids.indexOf(currentId ?? "");
  if (currentIndex < 0) {
    return ids[0];
  }

  const nextIndex = direction === "left" ? currentIndex - 1 : currentIndex + 1;
  return ids[Math.max(0, Math.min(ids.length - 1, nextIndex))];
}

export function createLibraryGamepadState(): LibraryGamepadState {
  return {
    axisDirection: null,
    navigationDirection: null,
    nextNavigationRepeatAt: null,
    buttons: {
      confirm: false,
      back: false,
      context: false,
      pageLeft: false,
      pageRight: false
    }
  };
}

function axisMagnitude(direction: LibraryDirection, horizontal: number, vertical: number) {
  if (direction === "left") {
    return Math.max(0, -horizontal);
  }
  if (direction === "right") {
    return Math.max(0, horizontal);
  }
  if (direction === "up") {
    return Math.max(0, -vertical);
  }
  return Math.max(0, vertical);
}

function resolvePressedAxisDirection(horizontal: number, vertical: number) {
  const horizontalMagnitude = Math.abs(horizontal);
  const verticalMagnitude = Math.abs(vertical);
  if (Math.max(horizontalMagnitude, verticalMagnitude) < LIBRARY_GAMEPAD_AXIS_PRESS_THRESHOLD) {
    return null;
  }
  if (horizontalMagnitude >= verticalMagnitude) {
    return horizontal < 0 ? "left" : "right";
  }
  return vertical < 0 ? "up" : "down";
}

export function resolveLibraryGamepadAxisDirection(
  axes: readonly number[],
  currentDirection: LibraryDirection | null = null
): LibraryDirection | null {
  const horizontal = Number.isFinite(axes[0]) ? axes[0] : 0;
  const vertical = Number.isFinite(axes[1]) ? axes[1] : 0;
  const pressedDirection = resolvePressedAxisDirection(horizontal, vertical);

  if (!currentDirection || axisMagnitude(currentDirection, horizontal, vertical) < LIBRARY_GAMEPAD_AXIS_RELEASE_THRESHOLD) {
    return pressedDirection;
  }
  if (!pressedDirection || pressedDirection === currentDirection) {
    return currentDirection;
  }

  const currentMagnitude = axisMagnitude(currentDirection, horizontal, vertical);
  const pressedMagnitude = axisMagnitude(pressedDirection, horizontal, vertical);
  return pressedMagnitude >= currentMagnitude + GAMEPAD_AXIS_SWITCH_MARGIN ? pressedDirection : currentDirection;
}

function isButtonPressed(button: LibraryGamepadButtonSnapshot) {
  if (typeof button === "boolean") {
    return button;
  }
  return Boolean(button && (button.pressed || button.value > 0.5));
}

function resolveDigitalDirection(buttons: readonly LibraryGamepadButtonSnapshot[], current: LibraryDirection | null) {
  const pressedDirections: LibraryDirection[] = [];
  if (isButtonPressed(buttons[GAMEPAD_BUTTON.up])) {
    pressedDirections.push("up");
  }
  if (isButtonPressed(buttons[GAMEPAD_BUTTON.down])) {
    pressedDirections.push("down");
  }
  if (isButtonPressed(buttons[GAMEPAD_BUTTON.left])) {
    pressedDirections.push("left");
  }
  if (isButtonPressed(buttons[GAMEPAD_BUTTON.right])) {
    pressedDirections.push("right");
  }
  return current && pressedDirections.includes(current) ? current : pressedDirections[0] ?? null;
}

export function stepLibraryGamepad(
  currentState: LibraryGamepadState,
  snapshot: LibraryGamepadSnapshot | null | undefined,
  now: number
): LibraryGamepadStepResult {
  if (!snapshot || snapshot.connected === false) {
    return { state: createLibraryGamepadState(), actions: [] };
  }

  const actions: LibraryNavigationAction[] = [];
  const axisDirection = resolveLibraryGamepadAxisDirection(snapshot.axes, currentState.axisDirection);
  const navigationDirection = resolveDigitalDirection(snapshot.buttons, currentState.navigationDirection) ?? axisDirection;
  let nextNavigationRepeatAt = currentState.nextNavigationRepeatAt;

  if (!navigationDirection) {
    nextNavigationRepeatAt = null;
  } else if (navigationDirection !== currentState.navigationDirection) {
    actions.push({ type: "move", direction: navigationDirection, source: "gamepad" });
    nextNavigationRepeatAt = now + LIBRARY_GAMEPAD_REPEAT_DELAY_MS;
  } else if (nextNavigationRepeatAt !== null && now >= nextNavigationRepeatAt) {
    actions.push({ type: "move", direction: navigationDirection, source: "gamepad" });
    nextNavigationRepeatAt = now + LIBRARY_GAMEPAD_REPEAT_INTERVAL_MS;
  }

  const buttons: LibraryGamepadButtonLatches = {
    confirm: isButtonPressed(snapshot.buttons[GAMEPAD_BUTTON.confirm]),
    back: isButtonPressed(snapshot.buttons[GAMEPAD_BUTTON.back]),
    context: isButtonPressed(snapshot.buttons[GAMEPAD_BUTTON.context]),
    pageLeft: isButtonPressed(snapshot.buttons[GAMEPAD_BUTTON.pageLeft]),
    pageRight: isButtonPressed(snapshot.buttons[GAMEPAD_BUTTON.pageRight])
  };

  if (buttons.pageLeft && !currentState.buttons.pageLeft) {
    actions.push({ type: "page", direction: "left", source: "gamepad" });
  }
  if (buttons.pageRight && !currentState.buttons.pageRight) {
    actions.push({ type: "page", direction: "right", source: "gamepad" });
  }
  if (buttons.confirm && !currentState.buttons.confirm) {
    actions.push({ type: "confirm", source: "gamepad" });
  }
  if (buttons.back && !currentState.buttons.back) {
    actions.push({ type: "back", source: "gamepad" });
  }
  if (buttons.context && !currentState.buttons.context) {
    actions.push({ type: "context", source: "gamepad" });
  }

  return {
    state: {
      axisDirection,
      navigationDirection,
      nextNavigationRepeatAt,
      buttons
    },
    actions
  };
}
