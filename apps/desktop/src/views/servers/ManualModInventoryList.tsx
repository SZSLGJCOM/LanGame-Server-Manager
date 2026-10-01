import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { ManualModInventoryItem } from "../../types";
import { formatWorkshopByteSize } from "./steam-workshop-store-model";

interface Props {
  items: ManualModInventoryItem[];
  selectedPath: string | null;
  canEnable: boolean;
  disabled: boolean;
  onSelect: (path: string) => void;
  onEnable: (item: ManualModInventoryItem) => void;
  canRemove?: (item: ManualModInventoryItem) => boolean;
  onRemove?: (item: ManualModInventoryItem) => void;
}

/** Instance-owned files remain visible even when the game has no enablement list. */
export function ManualModInventoryList(props: Props) {
  const { t } = useI18n();
  return <>{props.items.map((item) => <div key={item.path}
    className={props.selectedPath === item.path ? "mw-entry-row mw-entry-row--active" : "mw-entry-row"}>
    <button type="button" className="mw-entry-row-select" aria-pressed={props.selectedPath === item.path} onClick={() => props.onSelect(item.path)}>
      <span className="mw-entry-row-media mw-entry-row-media--file"><ShellIcon name="package" /></span>
      <span className="mw-entry-row-copy">
        <span className="mw-entry-row-title mw-entry-row-title--singleline">{item.name}</span>
        <span className="mw-entry-row-status">
          {item.inferred_id && item.inferred_id !== item.name ? <span className="mw-entry-id">{item.inferred_id}</span> : null}
          <span>{formatWorkshopByteSize(item.total_bytes)}</span>
        </span>
      </span>
    </button>
    {props.canEnable || props.canRemove?.(item) ? <div className="mw-entry-row-actions">
      {props.canEnable ? <input className="mw-entry-enabled-toggle" type="checkbox" checked={false}
        disabled={props.disabled || !item.inferred_id}
        aria-label={t("servers.mods.configList.enable", { name: item.name }, "Enable {name}")}
        onChange={() => { props.onSelect(item.path); props.onEnable(item); }} /> : null}
      {props.canRemove?.(item) ? <button type="button" className="mw-entry-remove-button" disabled={props.disabled}
        aria-label={t("servers.mods.configList.remove", { name: item.name }, "Remove {name} from this instance")}
        onClick={() => props.onRemove?.(item)}><ShellIcon name="x" className="mw-entry-remove-icon" /></button>
        : <span className="mw-entry-row-action-spacer" aria-hidden="true" />}
    </div> : null}
  </div>)}</>;
}
