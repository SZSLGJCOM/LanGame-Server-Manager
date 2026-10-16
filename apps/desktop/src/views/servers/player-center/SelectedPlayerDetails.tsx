import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RuntimeLivePlayerEntry, RuntimePlayerIdentityKind } from "../../../types";

export function formatPlayerSession(locale: LocaleCode, startedAt: number | null, now: number): string {
  if (startedAt === null) return selectLocaleText(locale, "未知", "Unknown");
  const elapsedMinutes = Math.max(0, Math.floor((now - startedAt) / 60_000));
  if (elapsedMinutes < 60) return selectLocaleText(locale, `${elapsedMinutes} 分钟`, `${elapsedMinutes} min`);
  const hours = Math.floor(elapsedMinutes / 60);
  const minutes = elapsedMinutes % 60;
  return selectLocaleText(locale, `${hours} 小时 ${minutes} 分`, `${hours}h ${minutes}m`);
}

function identityLabel(kind: RuntimePlayerIdentityKind, locale: LocaleCode): string {
  switch (kind) {
    case "steam_id": return "Steam ID";
    case "valheim_platform_id": return "Platform User ID";
    case "enshrouded_account_id": return selectLocaleText(locale, "原生 accountId", "Native accountId");
    case "eos_id": return "EOS ID";
    case "minecraft_uuid": return "Minecraft UUID";
    case "player_name": return selectLocaleText(locale, "玩家名称", "Player name");
    case "session_id": return selectLocaleText(locale, "会话 ID", "Session ID");
    default: return kind.replace(/_/g, " ");
  }
}

interface SelectedPlayerDetailsProps {
  locale: LocaleCode;
  now: number;
  player: RuntimeLivePlayerEntry;
  stale: boolean;
  sessionUnknownReason?: string;
}

/** Inspection does not grant runtime or roster authority. */
export function SelectedPlayerDetails({ locale, now, player, stale, sessionUnknownReason }: SelectedPlayerDetailsProps) {
  const identities = player.identifiers.filter((identity) => identity.kind !== "player_name" || identity.value !== player.display_name);
  return <section className="player-center-details" aria-label={selectLocaleText(locale, "玩家详情", "Player details")}>
    <div className="player-center-member-heading">
      <strong>{player.display_name}</strong>
      <span>{stale ? selectLocaleText(locale, "上次在线记录", "Last observed online") : selectLocaleText(locale, "当前在线", "Online")}</span>
    </div>
    <dl className="player-center-details-list">
      {identities.map((identity, index) => <div key={`${identity.kind}:${identity.value}:${index}`}>
        <dt>{identityLabel(identity.kind, locale)}{!identity.stable ? selectLocaleText(locale, "（会话）", " (session)") : ""}</dt>
        <dd className="server-player-cell--code">{identity.value}</dd>
      </div>)}
      <div>
        <dt>{selectLocaleText(locale, "在线时长", "Session")}</dt>
        <dd>{formatPlayerSession(locale, player.session_started_at_unix_ms, now)}
          {player.session_started_at_unix_ms === null ? <span className="player-center-detail-note">
            {sessionUnknownReason ?? selectLocaleText(locale, "玩家数据未提供本次会话的开始时间。", "Player data does not include the start of this session.")}
          </span> : null}
        </dd>
      </div>
      {player.ping_ms !== null ? <div><dt>{selectLocaleText(locale, "延迟", "Ping")}</dt><dd>{player.ping_ms} ms</dd></div> : null}
      {player.role ? <div><dt>{selectLocaleText(locale, "角色", "Role")}</dt><dd>{player.role}</dd></div> : null}
      {player.attributes.map((attribute, index) => <div key={`${attribute.key}:${index}`}>
        <dt>{attribute.key === "moderation_immune" ? selectLocaleText(locale, "受服务器保护", "Protected by server") : attribute.key}</dt>
        <dd>{attribute.key === "moderation_immune" && (attribute.value === "true" || attribute.value === "false")
          ? attribute.value === "true" ? selectLocaleText(locale, "是", "Yes") : selectLocaleText(locale, "否", "No") : attribute.value}</dd>
      </div>)}
    </dl>
  </section>;
}
