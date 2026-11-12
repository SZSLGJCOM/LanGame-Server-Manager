import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { ManualModInventoryItem } from "../../types";
import { formatWorkshopByteSize } from "./steam-workshop-store-model";
import { WorkshopStatus } from "./WorkshopStatus";
import { SteamWorkshopPreview } from "./SteamWorkshopStoreDetail";
import type { WorkshopDownloadItemPresentation } from "./workshop-download-presentation";

interface Props {
  items: ManualModInventoryItem[];
  selectedPath: string | null;
  canEnable: boolean;
  disabled: boolean;
  downloads?: Readonly<Record<string, WorkshopDownloadItemPresentation>>;
  onSelect: (path: string) => void;
  onEnable: (item: ManualModInventoryItem) => void;
  canRemove?: (item: ManualModInventoryItem) => boolean;
  onRemove?: (item: ManualModInventoryItem) => void;
  fileManagement?: boolean;
  onManagedToggle?: (item: ManualModInventoryItem) => void;
  onManagedDelete?: (item: ManualModInventoryItem) => void;
}

function canManageLocalMod(item: ManualModInventoryItem): boolean {
  if (/^(?:0_tfp_harmony|bepinex|plugins|mods|config|configs|cache|core|patchers|save|saves|settings|userdata)$/i.test(item.name) || item.name.startsWith(".")) return false;
  return item.item_type === "directory" || /\.(dll|jar|pak|lua|bundle|so|cs|smod|ucas|utoc)$/i.test(item.name);
}

/** Instance-owned files remain visible even when the game has no enablement list. */
export function ManualModInventoryList(props: Props) {
  const { t } = useI18n();
  return <>{props.items.map((item) => {
    const title = item.display_name ?? item.name;
    const fileControls = Boolean(props.fileManagement && (item.management || canManageLocalMod(item)));
    const enabled = item.management?.enabled !== false;
    return <div key={item.path}
    className={props.selectedPath === item.path ? "mw-entry-row mw-entry-row--active" : "mw-entry-row"}>
    <button type="button" className="mw-entry-row-select" aria-pressed={props.selectedPath === item.path} onClick={() => props.onSelect(item.path)}>
      <span className={item.icon_url ? "mw-entry-row-media" : "mw-entry-row-media mw-entry-row-media--file"}>
        {item.icon_url ? <SteamWorkshopPreview url={item.icon_url} loading /> : <ShellIcon name="package" />}
      </span>
      <span className="mw-entry-row-copy">
        <span className="mw-entry-row-title mw-entry-row-title--singleline">{title}</span>
        <span className="mw-entry-row-status">
          {props.downloads?.[item.path] ? <WorkshopStatus state={props.downloads[item.path].state} progressPercent={props.downloads[item.path].progressPercent} /> : null}
          {item.inferred_id && item.inferred_id !== item.name ? <span className="mw-entry-id">{item.inferred_id}</span> : null}
          <span>{formatWorkshopByteSize(item.total_bytes)}</span>
          {props.fileManagement ? <span>{fileControls
            ? enabled ? t("servers.mods.managed.enabled", undefined, "Enabled") : t("servers.mods.managed.disabled", undefined, "Disabled")
            : t("servers.mods.managed.unmanaged", undefined, "Local files")}</span> : null}
        </span>
      </span>
    </button>
    {fileControls ? <div className="mw-entry-row-actions">
        <input className="mw-entry-enabled-toggle" type="checkbox" checked={enabled} disabled={props.disabled}
          aria-label={enabled
            ? t("servers.mods.configList.disable", { name: item.name }, "Disable {name}")
            : t("servers.mods.configList.enable", { name: item.name }, "Enable {name}")}
          onChange={() => props.onManagedToggle?.(item)} />
        <button type="button" className="mw-entry-remove-button" disabled={props.disabled}
          aria-label={t("servers.mods.managed.deleteTitle", { name: item.name }, "Delete {name}?")}
          onClick={() => { props.onSelect(item.path); props.onManagedDelete?.(item); }}>
          <ShellIcon name="x" className="mw-entry-remove-icon" />
        </button>
    </div> : !props.fileManagement && (props.canEnable || props.canRemove?.(item)) ? <div className="mw-entry-row-actions">
      {props.canEnable ? <input className="mw-entry-enabled-toggle" type="checkbox" checked={false}
        disabled={props.disabled || !item.inferred_id}
        aria-label={t("servers.mods.configList.enable", { name: item.name }, "Enable {name}")}
        onChange={() => { props.onSelect(item.path); props.onEnable(item); }} /> : null}
      {props.canRemove?.(item) ? <button type="button" className="mw-entry-remove-button" disabled={props.disabled}
        aria-label={t("servers.mods.configList.remove", { name: item.name }, "Remove {name} from this instance")}
        onClick={() => props.onRemove?.(item)}><ShellIcon name="x" className="mw-entry-remove-icon" /></button>
        : <span className="mw-entry-row-action-spacer" aria-hidden="true" />}
    </div> : null}
  </div>;
  })}</>;
}
