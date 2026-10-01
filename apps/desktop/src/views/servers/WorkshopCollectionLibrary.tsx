import { useEffect, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { ActivityNotice } from "../../components/ActivityNotice";
import { SteamWorkshopPreview } from "./SteamWorkshopStoreDetail";
import { useI18n } from "../../i18n";
import type { SteamWorkshopLookupItem } from "../../types";
import { managedCollectionMemberIds, type ManagedWorkshopCollection } from "./mod-workbench-collections";
import "./workshop-collection-library.css";

export interface WorkshopCollectionMemberState {
  added: boolean;
  enabled: boolean;
  partiallyEnabled?: boolean;
  canToggle: boolean;
  canRemove: boolean;
}

interface Props {
  collections: ManagedWorkshopCollection[];
  selectedId: string | null;
  selectedMemberId: string | null;
  lookup: Record<string, SteamWorkshopLookupItem>;
  busy: boolean;
  readOnly?: boolean;
  enablementDisabled: boolean;
  loading: boolean;
  error: string | null;
  getMemberState: (id: string) => WorkshopCollectionMemberState;
  canRepair: (collection: ManagedWorkshopCollection) => boolean;
  onSelect: (id: string) => void;
  onOpenMember: (id: string, collectionId: string) => void;
  onToggleMembers: (ids: string[], enabled: boolean) => void;
  onRemoveMember: (id: string, collectionId: string) => void;
  onRepair: (collection: ManagedWorkshopCollection) => void;
  onRemove: (id: string) => void;
  onRetry: () => void;
}

export function WorkshopCollectionLibrary(props: Props) {
  const { t } = useI18n();
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set(props.selectedId ? [props.selectedId] : []));
  useEffect(() => {
    const id = props.selectedId;
    if (id) setExpandedIds((current) => new Set([...current, id]));
  }, [props.selectedId]);
  useEffect(() => {
    const retained = new Set(props.collections.map((entry) => entry.id));
    setExpandedIds((current) => new Set([...current].filter((id) => retained.has(id))));
  }, [props.collections]);
  function toggle(id: string) {
    if (!expandedIds.has(id)) props.onSelect(id);
    setExpandedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }
  return <>
    {props.error ? <ActivityNotice tone="error" action={<button type="button" className="mw-ghost-btn" onClick={props.onRetry} disabled={props.loading}>
      {t("common.retry", undefined, "Retry")}
    </button>}>{props.error}</ActivityNotice> : null}
    {props.collections.length === 0 ? <div className="mw-empty">
      <ShellIcon name="package" className="mw-empty-icon" />
      <span>{t("servers.mods.collections.empty", undefined, "No collections have been added to this instance.")}</span>
    </div> : <div className="mw-entry-list mw-collection-list">
      {props.collections.map((collection) => {
        const ids = managedCollectionMemberIds(collection, props.lookup[collection.id]);
        const members = ids.map((id) => ({ id, state: props.getMemberState(id) }));
        const added = members.filter(({ state }) => state.added).length;
        const toggleMembers = members.filter(({ state }) => state.added && state.canToggle);
        const toggleBlocked = members.some(({ state }) => state.added && !state.canToggle);
        const allEnabled = toggleMembers.length > 0 && toggleMembers.every(({ state }) => state.enabled);
        const mixed = !allEnabled && toggleMembers.some(({ state }) => state.enabled || state.partiallyEnabled);
        const name = props.lookup[collection.id]?.title || collection.title;
        const toggleLabel = allEnabled
          ? t("servers.mods.collections.disableMembers", { name }, "Disable Mods in {name}")
          : t("servers.mods.collections.enableMembers", { name }, "Enable Mods in {name}");
        const expanded = expandedIds.has(collection.id);
        const memberNames = new Map(props.lookup[collection.id]?.children.map((child) => [child.id, child.title]) ?? []);
        return <div key={collection.id} className="mw-collection-group">
          <div className={`mw-collection-row mw-entry-row${props.selectedId === collection.id ? " mw-entry-row--active" : ""}`}>
            <button type="button" className="mw-collection-select mw-entry-row-select" aria-pressed={props.selectedId === collection.id}
              aria-expanded={expanded} aria-controls={`collection-members-${collection.id}`} onClick={() => toggle(collection.id)}>
              <span className="mw-entry-row-media"><SteamWorkshopPreview url={props.lookup[collection.id]?.preview_url} loading={!props.readOnly} /></span>
              <span className="mw-entry-row-copy">
                <span className="mw-collection-title">
                  <ShellIcon name="chevron-right" className={expanded ? "mw-collection-chevron mw-collection-chevron--expanded" : "mw-collection-chevron"} />
                  <span className="mw-entry-row-title mw-entry-row-title--singleline">{name}</span>
                </span>
                {props.lookup[collection.id]?.description_excerpt ? <span className="mw-entry-row-excerpt">{props.lookup[collection.id].description_excerpt}</span> : null}
                <span className="mw-entry-row-status">
                  {ids.length ? <span className="mw-entry-row-meta mw-collection-count" aria-label={t("servers.mods.collections.memberCount", { added, count: ids.length }, "{added} of {count} Mods added")}>
                    {added === ids.length ? <ShellIcon name="check" className="mw-btn-icon" /> : null}{added} / {ids.length}
                  </span> : null}
                  <span className="mw-entry-id">#{collection.id}</span>
                </span>
              </span>
            </button>
            <div className="mw-entry-row-actions">
              {toggleMembers.length > 0 ? <input type="checkbox" className="mw-entry-enabled-toggle mw-collection-enabled-toggle"
                checked={allEnabled} aria-checked={mixed ? "mixed" : allEnabled}
                ref={(input) => { if (input) input.indeterminate = mixed; }}
                disabled={props.busy || props.enablementDisabled || toggleBlocked} aria-label={toggleLabel}
                title={toggleBlocked ? t("servers.mods.controls.localMetadata") : toggleLabel}
                onChange={() => props.onToggleMembers(toggleMembers.map(({ id }) => id), !allEnabled)} /> : null}
              {ids.length === 0 || added < ids.length ? <button type="button" className="mw-collection-repair-button" disabled={props.busy || props.enablementDisabled || !props.canRepair(collection)}
                aria-label={t("servers.mods.collections.addMissing", undefined, "Add missing Mods")}
                title={t("servers.mods.collections.addMissing", undefined, "Add missing Mods")} onClick={() => props.onRepair(collection)}>
                <ShellIcon name={props.busy ? "loader" : "plus"} className={props.busy ? "mw-btn-icon mw-btn-icon--spin" : "mw-btn-icon"} />
              </button> : null}
              <button type="button" className="mw-entry-remove-button" disabled={props.readOnly || props.busy}
                aria-label={t("servers.mods.collections.remove", { name }, "Remove collection: {name}")}
                title={t("servers.mods.collections.removeHint", undefined, "Review removal of this collection and its Mods from the instance.")}
                onClick={() => props.onRemove(collection.id)}><ShellIcon name="x" className="mw-btn-icon" /></button>
            </div>
          </div>
          <div id={`collection-members-${collection.id}`} className="mw-collection-members" role="region" aria-label={t("servers.mods.collections.members", undefined, "Collection members")} hidden={!expanded}>
            {members.map(({ id, state }) => {
              const title = props.lookup[id]?.title || memberNames.get(id);
              const selected = props.selectedId === collection.id && props.selectedMemberId === id;
              const memberName = title || id;
              const memberToggleLabel = state.enabled
                ? t("servers.mods.configList.disable", { name: memberName }, "Disable {name}")
                : t("servers.mods.configList.enable", { name: memberName }, "Enable {name}");
              const removeLabel = t("servers.mods.configList.remove", { name: memberName }, "Remove {name} from this instance");
              return <div key={id} className="mw-collection-member-row">
                <button type="button" className={selected ? "mw-collection-member mw-collection-member--active" : "mw-collection-member"}
                  aria-pressed={selected} onClick={() => props.onOpenMember(id, collection.id)}>
                  <ShellIcon name={state.added ? "check" : "plus"} className="mw-btn-icon" />
                  <span>{memberName}</span>{title && title !== id ? <small>#{id}</small> : null}
                </button>
                {state.added && (state.canToggle || state.canRemove) ? <div className="mw-collection-member-actions">
                  {state.canToggle ? <input type="checkbox" className="mw-entry-enabled-toggle" checked={state.enabled}
                    aria-checked={state.partiallyEnabled ? "mixed" : state.enabled}
                    ref={(input) => { if (input) input.indeterminate = Boolean(state.partiallyEnabled); }}
                    disabled={props.busy || props.enablementDisabled} aria-label={memberToggleLabel} title={memberToggleLabel}
                    onChange={() => props.onToggleMembers([id], !state.enabled)} /> : null}
                  {state.canRemove ? <button type="button" className="mw-entry-remove-button"
                    disabled={props.busy || props.enablementDisabled} aria-label={removeLabel} title={removeLabel}
                    onClick={() => props.onRemoveMember(id, collection.id)}><ShellIcon name="x" className="mw-entry-remove-icon" /></button> : null}
                </div> : null}
              </div>;
            })}
            {ids.length === 0 ? <div className="mw-empty mw-collection-members-empty">
              <span>{props.loading ? t("servers.mods.detailLoading", undefined, "Loading Mod details…")
                : t("servers.mods.collections.membersUnavailable", undefined, "No saved members are available. Reload collection details to inspect its Mods.")}</span>
              {!props.loading ? <button type="button" className="mw-ghost-btn" onClick={props.onRetry}>{t("common.retry", undefined, "Retry")}</button> : null}
            </div> : null}
          </div>
        </div>;
      })}
    </div>}
  </>;
}
