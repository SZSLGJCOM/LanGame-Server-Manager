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
  fileManagement?: boolean;
  onManagedToggle?: (item: ManualModInventoryItem) => void;
  onManagedDelete?: (item: ManualModInventoryItem) => void;
  onAdopt?: (item: ManualModInventoryItem) => void;
}

function canAdopt(item: ManualModInventoryItem): boolean {
  if (/^(?:0_tfp_harmony|bepinex|plugins|mods|config|configs|cache|core|patchers|save|saves|settings|userdata)$/i.test(item.name) || item.name.startsWith(".")) return false;
  return item.item_type === "directory" || /\.(dll|jar|pak|lua|bundle|so|cs|smod|ucas|utoc)$/i.test(item.name);
}

/** Instance-owned files remain visible even when the game has no enablement list. */
export function ManualModInventoryList(props: Props) {
  const { t } = useI18n();
  return <>{props.items.map((item) => <div key={item.path}
    className={`${props.selectedPath === item.path ? "mw-entry-row mw-entry-row--active" : "mw-entry-row"}${props.fileManagement && !item.management && canAdopt(item) ? " mw-entry-row--adopt" : ""}`}>
    <button type="button" className="mw-entry-row-select" aria-pressed={props.selectedPath === item.path} onClick={() => props.onSelect(item.path)}>
      <span className="mw-entry-row-media mw-entry-row-media--file"><ShellIcon name="package" /></span>
      <span className="mw-entry-row-copy">
        <span className="mw-entry-row-title mw-entry-row-title--singleline">{item.name}</span>
        <span className="mw-entry-row-status">
          {item.inferred_id && item.inferred_id !== item.name ? <span className="mw-entry-id">{item.inferred_id}</span> : null}
          <span>{formatWorkshopByteSize(item.total_bytes)}</span>
          {props.fileManagement ? <span>{item.management
            ? item.management.enabled ? t("servers.mods.managed.enabled", undefined, "Enabled") : t("servers.mods.managed.disabled", undefined, "Disabled")
            : t("servers.mods.managed.unmanaged", undefined, "Unmanaged files")}</span> : null}
        </span>
      </span>
    </button>
    {props.fileManagement ? <div className="mw-entry-row-actions">
      {item.management ? <>
        <input className="mw-entry-enabled-toggle" type="checkbox" checked={item.management.enabled} disabled={props.disabled}
          aria-label={item.management.enabled
            ? t("servers.mods.configList.disable", { name: item.name }, "Disable {name}")
            : t("servers.mods.configList.enable", { name: item.name }, "Enable {name}")}
          onChange={() => props.onManagedToggle?.(item)} />
        <button type="button" className="mw-entry-remove-button" disabled={props.disabled}
          aria-label={t("servers.mods.managed.deleteTitle", { name: item.name }, "Delete {name}?")}
          onClick={() => { props.onSelect(item.path); props.onManagedDelete?.(item); }}>
          <ShellIcon name="x" className="mw-entry-remove-icon" />
        </button>
      </> : canAdopt(item) ? <button type="button" className="mw-ghost-btn mw-managed-adopt" disabled={props.disabled}
        aria-label={t("servers.mods.managed.adoptName", { name: item.name }, "Manage {name}")}
        onClick={() => props.onAdopt?.(item)}>{t("servers.mods.managed.adopt", undefined, "Manage")}</button> : null}
    </div> : props.canEnable || props.canRemove?.(item) ? <div className="mw-entry-row-actions">
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
