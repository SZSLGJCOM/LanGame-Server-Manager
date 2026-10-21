import type { ModuleDetails, ModulePlayerActionDetails } from "./types";

// The manifest declares the native command and transport. Shutdown command
// lists may also stop the game and must never be treated as save capabilities.
export function immediateWorldSaveAction(module: ModuleDetails | null): ModulePlayerActionDetails | null {
  return module?.runtime?.player_actions?.find((action) => action.id === "save_world"
    && !action.target_required && !action.destructive && Boolean(action.command_template.trim())) ?? null;
}
