import { selectLocaleText, type LocaleCode, type TranslateFn } from "../../../i18n";
import type { RuntimeLivePlayerIssue } from "../../../types";

/** Explain structured player-management failures without interpreting native diagnostic text. */
export function enshroudedPlayerManagementUnavailableReason(
  issue: RuntimeLivePlayerIssue | null | undefined,
  locale: LocaleCode,
  t?: TranslateFn
): string | null {
  if (issue?.code === "protocol_incomplete") {
    const fallback = selectLocaleText(locale,
      "尚未取得完整有效的玩家身份，暂时无法踢出或封禁。请刷新玩家名单后重试。",
      "Complete, valid player identities are not available yet. Kick and ban are temporarily unavailable. Refresh the player list and try again.");
    return t ? t("servers.playerCenter.enshrouded.playerIdentityIncomplete", undefined, fallback) : fallback;
  }
  if (issue?.code !== "extension_unavailable") return null;
  const copy = issue.extension_status === "unsupported_game_build"
    ? ["unsupportedGameBuild", "当前 LanGame 尚未取得适配此游戏版本的玩家管理扩展，无法踢出或封禁。LanGame 会在实例启动时自动检查兼容更新。",
      "LanGame has no verified player-management extension for this Enshrouded game build. Kick and ban are unavailable. LanGame automatically checks for compatible updates when the instance starts."]
    : issue.extension_status === "not_installed"
      ? ["extensionNotInstalled", "玩家管理扩展尚未准备，无法踢出或封禁。请正常停服后通过 LanGame 启动，查看启动日志中的扩展准备结果。",
        "The player-management extension has not been prepared. Kick and ban are unavailable. Stop the server normally, start it through LanGame, and check the extension preparation result in the startup log."]
      : issue.extension_status === "loading"
        ? ["extensionLoading", "玩家管理扩展尚未就绪，暂时无法踢出或封禁。请等待服务器启动完成后刷新玩家名单。",
          "The player-management extension is not ready yet. Kick and ban are temporarily unavailable. Wait for server startup to finish, then refresh the player list."]
        : issue.extension_status === "load_failed"
          ? ["extensionLoadFailed", "玩家管理扩展加载失败，无法踢出或封禁。请查看服务器启动日志，正常停服后重新开服。",
            "The player-management extension failed to load. Kick and ban are unavailable. Check the server startup log, then stop the server normally and start it again."]
          : ["extensionNotReady", "玩家管理扩展未就绪，无法踢出或封禁。请查看服务器启动日志，确认扩展已准备并加载后刷新玩家名单。",
            "The player-management extension is not ready. Kick and ban are unavailable. Check the server startup log and refresh the player list once the extension has been prepared and loaded."];
  const fallback = selectLocaleText(locale, copy[1], copy[2]);
  return t ? t(`servers.playerCenter.enshrouded.${copy[0]}`, undefined, fallback) : fallback;
}
