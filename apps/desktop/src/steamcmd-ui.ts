import { message, type UiMessage } from "./app-ui";
import type { SteamCmdStatus } from "./types";

export function steamCmdDetailMessage(status: SteamCmdStatus): UiMessage {
  if (status.ownership === "invalid") {
    return message("activity.steamCmdOwnershipInvalidAt", { path: status.root });
  }

  if (status.ready) {
    if (status.ownership === "managed") {
      return message("activity.steamCmdReadyAt", { path: status.executable_path });
    }
    return message("activity.steamCmdDetectedAt", { path: status.executable_path });
  }

  return message(status.executable_exists ? "activity.steamCmdNotReadyAt" : "activity.steamCmdMissingAt", {
    path: status.executable_exists ? status.executable_path : status.configured_executable_path || status.executable_path
  });
}

export function steamCmdSummaryMessage(status: SteamCmdStatus): UiMessage {
  if (status.ownership === "invalid") {
    return message("activity.steamCmdOwnershipInvalid");
  }

  if (status.ready) {
    if (status.ownership === "managed") {
      return message("activity.steamCmdReady");
    }
    return message("activity.steamCmdDetected");
  }

  return message(status.executable_exists ? "system.steamCmdNotReady" : "activity.steamCmdMissing");
}
