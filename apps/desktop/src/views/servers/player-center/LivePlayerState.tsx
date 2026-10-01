import { ShellIcon, type ShellIconName } from "../../../components/ShellIcon";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { LivePlayerPresentation } from "../../../domain/live-player-state";
import type { RuntimeLivePlayerSnapshot } from "../../../types";

interface LivePlayerStateProps {
  readOnly?: boolean;
  error: string | null;
  locale: LocaleCode;
  moduleId?: string;
  onOpenSettings?: () => void;
  presentation: LivePlayerPresentation;
  snapshot: RuntimeLivePlayerSnapshot | null;
}

function localizedIssueCopy(
  locale: LocaleCode,
  snapshot: RuntimeLivePlayerSnapshot | null,
  fallbackZh: string,
  fallbackEn: string,
  moduleId?: string
): string {
  switch (snapshot?.issue?.code) {
    case "process_unavailable":
      return selectLocaleText(locale, "玩家采集进程当前不可用。", "The player collection process is unavailable.");
    case "process_untracked":
      return selectLocaleText(locale, "服务器仍在运行，但当前 LanGame 会话未接管。请通过 LanGame 正常停服后重新启动，以恢复玩家查询。", "The server is running, but this LanGame session is not supervising it. Stop it normally in LanGame, then start it again to restore player queries.");
    case "log_unavailable":
      return selectLocaleText(locale, "服务器尚未提供可读取的玩家日志。", "The server has not provided a readable player log.");
    case "collection_timeout":
      return selectLocaleText(locale, "服务器没有在限定时间内返回完整玩家列表。", "The server did not return a complete player list before the timeout.");
    case "protocol_incomplete":
      return selectLocaleText(locale, "服务器返回的玩家列表不完整。", "The server returned an incomplete player list.");
    case "capture_limit":
      return selectLocaleText(locale, "玩家列表超过安全采集上限，当前结果已截断。", "The player list exceeded the safe collection limit and was truncated.");
    case "io_failed":
      if (snapshot.issue.setting_keys.includes("console_password")) {
        return selectLocaleText(locale, "控制台在返回名单前关闭了连接。请检查控制台密码和服务器运行状态。", "The console closed before returning players. Check the console password and server status.");
      }
      return selectLocaleText(locale, "读取玩家快照时发生本地 I/O 错误。", "A local I/O error occurred while reading the player snapshot.");
    case "runtime_action_unavailable":
      return selectLocaleText(locale, "模块声明的玩家查询操作当前不可用，请检查服务器配置。", "The module-declared player query is unavailable; check the server settings.");
    case "adapter_unavailable":
      return selectLocaleText(locale, "LanGame 尚未接入此服务器的在线玩家读取通道。已取得的在线人数会继续显示。", "LanGame has not connected a player-list adapter for this server. Available player counts remain visible.");
    case "names_unavailable":
      return selectLocaleText(locale, "本次查询未返回完整的玩家名称。已确认的在线人数会继续显示。", "This query did not return every player name. Confirmed player counts remain visible.");
    case "query_unavailable":
      if (moduleId === "valheim" && snapshot.issue.setting_keys.includes("public_server")) {
        return selectLocaleText(locale, "Valheim 的私密服务器不提供 Steam 玩家查询。读取玩家列表需要将浏览器可见性设为公开。", "Private Valheim servers do not expose the Steam player query. Public browser visibility is required to read the player list.");
      }
      if (moduleId === "vrising" && snapshot.issue.setting_keys.includes("list_on_steam")) {
        return selectLocaleText(locale, "V Rising 关闭“列入 Steam 列表”后不提供 Steam 玩家查询。启用此设置后可查询；玩家仍可直接连接服务器。", "V Rising does not expose Steam player queries while List On Steam is disabled. Enable it to query players; direct game connections remain available.");
      }
      if (moduleId === "abioticfactor" && snapshot.issue.setting_keys.includes("lan_only")) {
        return selectLocaleText(locale, "Abiotic Factor 的“仅限局域网”模式不提供 Steam 玩家查询。局域网玩家仍可在游戏内发现服务器。", "Abiotic Factor does not expose Steam player queries in LAN Only mode. LAN players can still discover the server in-game.");
      }
      return selectLocaleText(locale, "玩家查询端口未响应，请检查端口、查询开关和服务器运行状态。", "The player query did not respond. Check the query port, query setting, and server status.");
    case "authentication_failed":
      return selectLocaleText(locale, "玩家查询认证失败，请检查远程控制或 API 凭据。", "Player query authentication failed. Check the remote-control or API credentials.");
    case "extension_unavailable":
      if (moduleId === "runescapedragonwilds") {
        return selectLocaleText(locale, "Dragonwilds 的玩家查询扩展未就绪。请安装兼容的 UE4SS 加载器、专服代理和 LgsmPlayerQuery，重启服务器后刷新。", "The Dragonwilds player-query extension is unavailable. Install a compatible UE4SS loader, dedicated-server proxy and LgsmPlayerQuery, restart the server, and refresh.");
      }
      if (moduleId === "windrose") {
        return selectLocaleText(locale, "Windrose 的玩家查询扩展未就绪。请安装兼容的 UE4SS 加载器并启用 LgsmPlayerQuery，重启服务器后刷新。", "The Windrose player-query extension is unavailable. Install a compatible UE4SS loader, enable LgsmPlayerQuery, restart the server, and refresh.");
      }
      if (moduleId === "satisfactory") {
        return selectLocaleText(locale, "Satisfactory 的 FRM 玩家查询未就绪。请安装兼容的 Ficsit Remote Monitoring 模组，在游戏的服务器管理器中启用 FRM HTTP 自动启动，重启服务器后刷新。", "The Satisfactory FRM player query is unavailable. Install a compatible Ficsit Remote Monitoring mod, enable FRM HTTP autostart in the game's Server Manager, restart the server, and refresh.");
      }
      return selectLocaleText(locale, "服务器的玩家查询扩展未就绪，请检查扩展配置和服务器运行状态。", "The player-query extension is unavailable. Check the extension configuration and server status.");
    default:
      return selectLocaleText(locale, fallbackZh, fallbackEn);
  }
}

function stateCopy(
  locale: LocaleCode,
  presentation: LivePlayerPresentation,
  snapshot: RuntimeLivePlayerSnapshot | null,
  error: string | null,
  moduleId?: string
): { icon: ShellIconName; title: string; copy: string } {
  if (error) {
    return {
      icon: "alert-circle",
      title: selectLocaleText(locale, "读取在线玩家失败", "Could not read online players"),
      copy: error
    };
  }
  switch (presentation.kind) {
    case "empty":
      return { icon: "inbox", title: selectLocaleText(locale, "当前没有在线玩家", "No players are online"), copy: selectLocaleText(locale, "这是服务器返回的完整空结果。", "The server returned a complete empty result.") };
    case "stopped":
      return { icon: "stop-circle", title: selectLocaleText(locale, "服务器尚未运行", "Server is not running"), copy: selectLocaleText(locale, "启动服务器后可刷新玩家信息。", "Start the server to refresh player information.") };
    case "adapter-unavailable":
      return { icon: "users", title: selectLocaleText(locale, "在线玩家读取尚未接入", "Player-list adapter unavailable"), copy: localizedIssueCopy(locale, snapshot, "LanGame 尚未接入此服务器的在线玩家读取通道。", "LanGame has not connected a player-list adapter for this server.") };
    case "count-only":
      return { icon: "users", title: selectLocaleText(locale, "已取得在线人数，暂无玩家名称", "Player count available; names unavailable"), copy: localizedIssueCopy(locale, snapshot, "本次查询未返回可显示的玩家名称。", "This query did not return displayable player names.") };
    case "unsupported":
      return { icon: "users", title: selectLocaleText(locale, "暂未取得在线玩家列表", "Online player list unavailable"), copy: localizedIssueCopy(locale, snapshot, "当前连接未提供可读取的玩家列表。已取得的在线人数会继续显示。", "The current connection has no readable player list. Available player counts remain visible.", moduleId) };
    case "misconfigured":
      return {
        icon: "settings",
        title: selectLocaleText(locale, "玩家查询配置不完整", "Player query is not configured"),
        copy: localizedIssueCopy(
          locale,
          snapshot,
          "请补齐服务器要求的远程控制或查询设置。",
          "Complete the remote-control or query settings required by this server.",
          moduleId
        )
      };
    case "failed-with-rows":
    case "failed-without-rows":
      return {
        icon: "alert-circle",
        title: selectLocaleText(locale, "玩家快照刷新失败", "Player snapshot refresh failed"),
        copy: localizedIssueCopy(
          locale,
          snapshot,
          "请重试；旧数据会明确标记，不会当作实时结果。",
          "Retry the request; retained rows are marked as stale and are never treated as current.",
          moduleId
        )
      };
    case "truncated":
      return { icon: "alert-triangle", title: selectLocaleText(locale, "玩家列表已截断", "Player list was truncated"), copy: selectLocaleText(locale, "已显示取得的条目，但结果不完整，成员操作已停用。", "Available rows are shown, but the result is incomplete and member actions are disabled.") };
    case "incomplete":
      return { icon: "alert-triangle", title: selectLocaleText(locale, "玩家列表不完整", "Player list is incomplete"), copy: localizedIssueCopy(locale, snapshot, "服务器响应未完整结束，不能据此确认无人在线或执行成员操作。", "The response did not complete, so it cannot prove an empty server or authorize member actions.") };
    case "refreshing-with-rows":
    case "refreshing-without-rows":
      return { icon: "refresh", title: selectLocaleText(locale, "正在刷新玩家快照", "Refreshing player snapshot"), copy: selectLocaleText(locale, "等待服务器返回在线玩家信息。", "Waiting for online player information from the server.") };
    default:
      return { icon: "users", title: selectLocaleText(locale, "正在读取在线玩家", "Loading online players"), copy: selectLocaleText(locale, "这里只显示服务器能够可靠确认的成员。", "Only members reliably confirmed by the server are shown.") };
  }
}

export function LivePlayerState(props: LivePlayerStateProps) {
  const copy = props.readOnly ? {
    icon: "users" as const,
    title: selectLocaleText(props.locale, "实例已归档", "Instance archived"),
    copy: selectLocaleText(props.locale, "还原并启动服务器后可查看在线玩家。", "Restore and start the server to view online players.")
  } : stateCopy(props.locale, props.presentation, props.snapshot, props.error, props.moduleId);
  const compact = props.presentation.tableVisible;
  return (
    <div className={`player-center-state${compact ? " is-compact" : ""}`} role={props.error ? "alert" : "status"}>
      <span className="player-center-state-icon" aria-hidden="true"><ShellIcon name={copy.icon} /></span>
      <div>
        <strong>{copy.title}</strong>
        <p>{copy.copy}</p>
      </div>
      {(props.presentation.kind === "misconfigured" || props.snapshot?.issue?.code === "authentication_failed" || (props.snapshot?.issue?.setting_keys.length ?? 0) > 0) && props.onOpenSettings ? (
        <button type="button" className="secondary-button" onClick={props.onOpenSettings}>
          {selectLocaleText(props.locale, "打开配置", "Open settings")}
        </button>
      ) : null}
    </div>
  );
}
