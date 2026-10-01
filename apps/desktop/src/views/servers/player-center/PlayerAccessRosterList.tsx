import { ShellIcon } from "../../../components/ShellIcon";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { RosterEntry, RosterField } from "./player-access-roster-model";

interface PlayerAccessRosterListProps {
  field: RosterField;
  locale: LocaleCode;
  selectedEntryKey: string | null;
  onSelect: (entry: RosterEntry) => void;
  disabled: boolean;
}

export function PlayerAccessRosterList(props: PlayerAccessRosterListProps) {
  return <div className="player-access-roster-list">
    <div className="player-access-roster-entries" role="list"
      aria-label={selectLocaleText(props.locale, `${props.field.title} 当前条目`, `Current ${props.field.title} entries`)}>
      {props.field.entries.length > 0 ? props.field.entries.map((entry) => {
        const selected = entry.key === props.selectedEntryKey;
        return <div key={entry.key} className={`player-access-roster-entry${selected ? " is-selected" : ""}`} role="listitem">
          <button type="button" className="player-center-player-select player-access-roster-select"
            aria-pressed={selected} disabled={props.disabled} onClick={() => props.onSelect(entry)}
            aria-label={selectLocaleText(props.locale, `选择条目 ${entry.label}`, `Select entry ${entry.label}`)}>
            <span className="player-access-roster-entry-value" title={entry.label}>{entry.label}</span>
          </button>
        </div>;
      }) : <div className="player-access-roster-empty" role="status">
        <ShellIcon name="inbox" className="player-access-roster-empty-icon" />
        <span>{selectLocaleText(props.locale, "暂无记录", "No entries")}</span>
      </div>}
    </div>
  </div>;
}
