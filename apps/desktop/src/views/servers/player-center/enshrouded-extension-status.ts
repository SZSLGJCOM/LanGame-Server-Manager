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
    ? ["unsupportedGameBuild", "当前版本的玩家管理扩展暂不可用，不影响正常游玩。开服后会在后台检查已发布的兼容更新，下载后于下次正常开服时使用。",
      "Player management is unavailable for this game build; normal gameplay is unaffected. Published compatible updates are checked in the background after startup and used on the next normal start."]
    : issue.extension_status === "not_installed"
      ? ["extensionNotInstalled", "玩家管理扩展尚未准备，无法踢出或封禁，不影响正常游玩。请查看启动日志；下次正常开服时会重新准备。",
        "The player-management extension has not been prepared. Kick and ban are unavailable; normal gameplay is unaffected. Check the startup log. Preparation will be retried on the next normal start."]
      : issue.extension_status === "loading"
        ? ["extensionLoading", "玩家管理扩展尚未就绪，暂时无法踢出或封禁。请等待服务器启动完成后刷新玩家名单。",
          "The player-management extension is not ready yet. Kick and ban are temporarily unavailable. Wait for server startup to finish, then refresh the player list."]
        : issue.extension_status === "load_failed"
          ? ["extensionLoadFailed", "玩家管理扩展加载失败，无法踢出或封禁，不影响正常游玩。请查看启动日志，无需为此中断服务器。",
            "The player-management extension failed to load. Kick and ban are unavailable; normal gameplay is unaffected. Check the startup log. There is no need to interrupt the server."]
          : ["extensionNotReady", "玩家管理扩展未就绪，无法踢出或封禁。请查看服务器启动日志，确认扩展已准备并加载后刷新玩家名单。",
            "The player-management extension is not ready. Kick and ban are unavailable. Check the server startup log and refresh the player list once the extension has been prepared and loaded."];
  const fallback = selectLocaleText(locale, copy[1], copy[2]);
  return t ? t(`servers.playerCenter.enshrouded.${copy[0]}`, undefined, fallback) : fallback;
}
