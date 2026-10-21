import { mapPlayerArtwork, type DstMapArt, type DstMapPlayer } from "../../dst-world-map";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";

interface Props {
  players: readonly DstMapPlayer[]; art: DstMapArt | null; selectedId: string | null;
  /** Why the strip is empty; null (including no players online) shows nothing in the strip's reserved height. */
  empty: string | null;
  /** Shown only while the selected player is being looked for on another shard. */
  status: string | null;
  onToggle: (player: DstMapPlayer) => void; onClear: () => void;
}

/** One slim chip strip shared by both GM panels; selecting a chip (or a map marker) follows that player and targets player actions. */
export function DstMapRoster({ players, art, selectedId, empty, status, onToggle, onClear }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  return <section className="dst-map-roster" aria-label={text("在线玩家", "Players")}>
    <ul>{players.map(player => {
      const avatar = mapPlayerArtwork(art, player.icon);
      return <li key={player.id}><button type="button" aria-pressed={selectedId === player.id}
        title={`${player.character} · ${player.id} · ${player.x.toFixed(0)}, ${player.z.toFixed(0)}`} onClick={() => onToggle(player)}>
        {avatar ? <img src={avatar} alt="" /> : <ShellIcon name="user" />}<strong>{player.name || player.id}</strong>
      </button></li>;
    })}</ul>
    {players.length || !empty ? null : <p>{empty}</p>}
    {selectedId ? <button type="button" className="dst-map-roster-clear" onClick={onClear} title={text("取消选择", "Clear selection")}
      aria-label={text("取消选择", "Clear selection")}><ShellIcon name="x" /></button> : null}
    {status ? <p role="status">{status}</p> : null}
  </section>;
}
