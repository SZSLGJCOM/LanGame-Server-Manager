import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RuntimeLivePlayerEntry } from "../../../types";
import type { PlayerAccessRosterCapability } from "./player-access-roster-model";
import { resolveSelectedRosterIdentity } from "./player-access-selected-target";

interface LivePlayerTableProps {
  locale: LocaleCode;
  now: number;
  onSelect: (playerKey: string) => void;
  rows: RuntimeLivePlayerEntry[];
  rosterCapabilities?: PlayerAccessRosterCapability[];
  selectedPlayerKey: string | null;
  stale?: boolean;
}

function formatSessionStarted(locale: LocaleCode, startedAt: number | null, now: number): string {
  if (startedAt === null) {
    return selectLocaleText(locale, "未知", "Unknown");
  }
  const elapsedMinutes = Math.max(0, Math.floor((now - startedAt) / 60_000));
  if (elapsedMinutes < 60) {
    return selectLocaleText(locale, `${elapsedMinutes} 分钟`, `${elapsedMinutes} min`);
  }
  const hours = Math.floor(elapsedMinutes / 60);
  const minutes = elapsedMinutes % 60;
  return selectLocaleText(locale, `${hours} 小时 ${minutes} 分`, `${hours}h ${minutes}m`);
}

export function LivePlayerTable(props: LivePlayerTableProps) {
  const showPing = props.rows.some((entry) => entry.ping_ms !== null);
  const showRole = props.rows.some((entry) => Boolean(entry.role));
  const showIdentity = props.rows.some((entry) => entry.identifiers.some((identity) => identity.stable));

  return (
    <div className="player-center-table-scroll">
      <table className="player-center-player-table">
        <thead>
          <tr>
            <th scope="col">{selectLocaleText(props.locale, "玩家", "Player")}</th>
            {showIdentity ? <th scope="col">{selectLocaleText(props.locale, "账号身份", "Identity")}</th> : null}
            <th scope="col">{selectLocaleText(props.locale, "在线时长", "Session")}</th>
            {showPing ? <th scope="col">{selectLocaleText(props.locale, "延迟", "Ping")}</th> : null}
            {showRole ? <th scope="col">{selectLocaleText(props.locale, "角色", "Role")}</th> : null}
            <th scope="col">{selectLocaleText(props.locale, "状态", "Status")}</th>
          </tr>
        </thead>
        <tbody>
          {props.rows.map((player) => {
            const selected = player.player_key === props.selectedPlayerKey;
            const primaryIdentity = player.identifiers.find((identity) => identity.stable) ?? null;
            const subtitle = player.attributes[0]?.value ?? null;
            const selectable = player.available_action_ids.length > 0
              || props.rosterCapabilities?.some((field) => resolveSelectedRosterIdentity(player, field) !== null);
            const profile = (
              <span className="server-player-profile-copy">
                <strong className="server-player-name" title={player.display_name}>{player.display_name}</strong>
                {subtitle ? <span className="server-player-cell--muted" title={subtitle}>{subtitle}</span> : null}
              </span>
            );
            return (
              <tr key={player.player_key} className={selected ? "is-selected" : ""}>
                <td>
                  {selectable ? <button
                    type="button"
                    className="player-center-player-select"
                    aria-label={selectLocaleText(props.locale, `选择玩家 ${player.display_name}`, `Select player ${player.display_name}`)}
                    aria-pressed={selected}
                    onClick={() => props.onSelect(player.player_key)}
                  >
                    {profile}
                  </button> : profile}
                </td>
                {showIdentity ? <td>
                  {primaryIdentity ? (
                    <span className="server-player-cell--code" title={`${primaryIdentity.kind}: ${primaryIdentity.value}`}>
                      {primaryIdentity.value}
                    </span>
                  ) : <span className="server-player-cell--muted">—</span>}
                </td> : null}
                <td>{formatSessionStarted(props.locale, player.session_started_at_unix_ms, props.now)}</td>
                {showPing ? <td>{player.ping_ms === null ? "—" : `${player.ping_ms} ms`}</td> : null}
                {showRole ? <td>{player.role || "—"}</td> : null}
                <td><span className="server-player-role-chip server-player-role-chip--muted">{props.stale
                  ? selectLocaleText(props.locale, "上次在线", "Last observed online")
                  : selectLocaleText(props.locale, "在线", "Online")}</span></td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
