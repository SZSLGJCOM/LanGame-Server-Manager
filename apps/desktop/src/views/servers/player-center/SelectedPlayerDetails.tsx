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
  statusLabel?: string;
  hideSession?: boolean;
}

/**
 * Right-panel variant: only what the selected list row does not already show (secondary identities,
 * attributes beyond the row subtitle, server protection, why the session time is unknown). Null when nothing is unique.
 */
export function SelectedPlayerExtraDetails({ locale, player, sessionUnknownReason }: Pick<SelectedPlayerDetailsProps, "locale" | "player" | "sessionUnknownReason">) {
  // Mirrors LivePlayerTable: the row shows the first stable identity and the first non-protection attribute.
  const primary = player.identifiers.find((identity) => identity.stable) ?? null;
  const subtitle = player.attributes.find((attribute) => attribute.key !== "moderation_immune") ?? null;
  const identities = player.identifiers.filter((identity) => identity !== primary
    && (identity.kind !== "player_name" || identity.value !== player.display_name));
  const attributes = player.attributes.filter((attribute) => attribute !== subtitle);
  const sessionNote = player.session_started_at_unix_ms === null ? sessionUnknownReason ?? null : null;
  if (identities.length === 0 && attributes.length === 0 && !sessionNote) return null;
  return <section className="player-center-details is-compact" aria-label={selectLocaleText(locale, "\u5176\u4ed6\u8be6\u60c5", "More details")}>
    {identities.length > 0 || attributes.length > 0 ? <dl className="player-center-details-list">
      {identities.map((identity, index) => <div key={`${identity.kind}:${identity.value}:${index}`}>
        <dt>{identityLabel(identity.kind, locale)}{!identity.stable ? selectLocaleText(locale, "\uff08\u4f1a\u8bdd\uff09", " (session)") : ""}</dt>
        <dd className="server-player-cell--code">{identity.value}</dd>
      </div>)}
      {attributes.map((attribute, index) => <div key={`${attribute.key}:${index}`}>
        <dt>{attribute.key === "moderation_immune" ? selectLocaleText(locale, "\u53d7\u670d\u52a1\u5668\u4fdd\u62a4", "Protected by server") : attribute.key}</dt>
        <dd>{attribute.key === "moderation_immune" && (attribute.value === "true" || attribute.value === "false")
          ? attribute.value === "true" ? selectLocaleText(locale, "\u662f", "Yes") : selectLocaleText(locale, "\u5426", "No") : attribute.value}</dd>
      </div>)}
    </dl> : null}
    {sessionNote ? <p className="player-center-detail-note">{sessionNote}</p> : null}
  </section>;
}

/** Inspection does not grant runtime or roster authority. */
export function SelectedPlayerDetails({ locale, now, player, stale, sessionUnknownReason, statusLabel, hideSession }: SelectedPlayerDetailsProps) {
  const identities = player.identifiers.filter((identity) => identity.kind !== "player_name" || identity.value !== player.display_name);
  return <section className="player-center-details" aria-label={selectLocaleText(locale, "玩家详情", "Player details")}>
    <div className="player-center-member-heading">
      <strong>{player.display_name}</strong>
      <span>{statusLabel ?? (stale ? selectLocaleText(locale, "上次在线记录", "Last observed online") : selectLocaleText(locale, "当前在线", "Online"))}</span>
    </div>
    <dl className="player-center-details-list">
      {identities.map((identity, index) => <div key={`${identity.kind}:${identity.value}:${index}`}>
        <dt>{identityLabel(identity.kind, locale)}{!identity.stable ? selectLocaleText(locale, "（会话）", " (session)") : ""}</dt>
        <dd className="server-player-cell--code">{identity.value}</dd>
      </div>)}
      {!hideSession ? <div>
        <dt>{selectLocaleText(locale, "在线时长", "Session")}</dt>
        <dd>{formatPlayerSession(locale, player.session_started_at_unix_ms, now)}
          {player.session_started_at_unix_ms === null ? <span className="player-center-detail-note">
            {sessionUnknownReason ?? selectLocaleText(locale, "玩家数据未提供本次会话的开始时间。", "Player data does not include the start of this session.")}
          </span> : null}
        </dd>
      </div> : null}
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
