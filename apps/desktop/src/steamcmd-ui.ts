import { message, type UiMessage } from "./app-ui";
import type { TranslateFn } from "./i18n";
import type { SteamCmdStatus } from "./types";

export type SteamCmdTone = "" | "is-success" | "is-busy" | "is-danger";

export interface SteamCmdStatusPresentation {
  label: string;
  tone: SteamCmdTone;
}

export function describeSteamCmdStatus(status: SteamCmdStatus | null, t: TranslateFn): SteamCmdStatusPresentation {
  if (!status) {
    return {
      label: t("system.steamCmdChecking"),
      tone: "is-busy"
    };
  }

  if (status.ownership === "invalid") {
    return {
      label: t("system.steamCmdOwnershipInvalid"),
      tone: "is-danger"
    };
  }

  if (status.ready) {
    return {
      label: status.ownership === "managed" ? t("system.steamCmdReady") : t("system.steamCmdDetected"),
      tone: "is-success"
    };
  }

  return {
    label: t(status.executable_exists ? "system.steamCmdNotReady" : "system.steamCmdMissing"),
    tone: "is-danger"
  };
}

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
