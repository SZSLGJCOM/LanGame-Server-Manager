import React, { useState } from "react";
import { updateInstance } from "../../src/api";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, SteamWorkshopBrowseKind,
  SteamWorkshopLookupItem, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

interface HostProps { details: InstanceDetails; moduleDetails: ModuleDetails; epoch: number; onSaved: () => void }
function Workspace(props: HostProps) {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div style={{ flex: 1, minHeight: 0 }}>
      <ModWorkbench key={`${props.epoch}:${props.details.summary.id}`} details={props.details} moduleDetails={props.moduleDetails}
        launchPlan={null} onSaveSettings={async (input, options) => {
          const saved = await updateInstance(input, options?.expectedSettingsJson ?? "", options?.collectionRemoval);
          props.onSaved();
          return saved;
        }} />
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
export function ModWorkbenchBrowserHost(props: HostProps) {
  return <I18nProvider><InstanceSettingsSaveProvider key={props.epoch}><Workspace {...props} /></InstanceSettingsSaveProvider></I18nProvider>;
}

export function workshopItem(id: string, title: string): SteamWorkshopLookupItem {
  return { id, title, item_kind: "item", status: "resolved", consumer_app_id: 322330,
    detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`, child_count: 0, children: [],
    description_excerpt: "A synthetic server Mod used only by this isolated browser fixture.", tags: ["Server"] };
}
interface State {
  details: InstanceDetails; moduleDetails: ModuleDetails; catalog: SteamWorkshopLookupItem[];
  cached: Set<string>; inventory?: ManualModInventoryResult | null; pageSize?: number;
}
export interface FixtureBrowseRequest { kind: SteamWorkshopBrowseKind; query: string; sort: string; page: number }
interface BoundaryHooks {
  check: (condition: unknown, label: string) => void;
  save: (details: InstanceDetails) => void;
  download?: (ids: string[]) => void;
  openPath?: (path: string) => void;
  browse?: (request: FixtureBrowseRequest, result: Record<string, unknown>) => unknown | Promise<unknown>;
}
/** Replace only native IPC. Rendering, ownership rules, plans and saves remain real. */
export function createModWorkbenchFixtureIpc(readState: () => State, hooks: BoundaryHooks) {
  return async (command: string, payload?: unknown): Promise<unknown> => {
    const args = (payload ?? {}) as Record<string, unknown>;
    const state = readState();
    const ids = (args.ids ?? []) as string[];
    const appId = state.moduleDetails.workshop?.consumer_app_id ?? 322330;
    switch (command) {
      case "read_instance_details_from_storage": return structuredClone(state.details);
      case "read_background_jobs": return [];
      case "read_manual_mod_inventory":
        hooks.check(args.instanceId === state.details.summary.id, "Inventory read stays scoped to its instance");
        if (!state.inventory) throw new Error("Unexpected inventory read");
        return structuredClone(state.inventory);
      case "open_local_path": hooks.openPath?.(String(args.path)); return;
      case "lookup_steam_workshop_items": return ids.map((id) => state.catalog.find((item) => item.id === id) ?? workshopItem(id, `Mod ${id}`));
      case "read_steam_workshop_item_details": return state.catalog.find((item) => item.id === String(args.id)) ?? workshopItem(String(args.id), `Mod ${args.id}`);
      case "search_steam_workshop_items": {
        const kind = args.browseKind;
        if (kind !== "item" && kind !== "collection") throw new Error("Browse did not send a supported content kind");
        hooks.check(true, "Browse sends an explicit supported content kind");
        const request: FixtureBrowseRequest = { kind, query: String(args.query ?? ""), sort: String(args.sort), page: Number(args.page) };
        const matching = state.catalog.filter((item) => item.item_kind === kind &&
          (item.title ?? "").toLowerCase().includes(request.query.toLowerCase()));
        const pageSize = state.pageSize ?? 12;
        const items = matching.slice((request.page - 1) * pageSize, request.page * pageSize)
          .map((item) => item.item_kind === "collection" ? { ...item, children: [] } : item);
        const result = { app_id: appId, browse_kind: kind, items, page: request.page, page_size: pageSize,
          has_more: request.page * pageSize < matching.length, total_count: matching.length,
          query: request.query, sort: request.sort, source_url: "" };
        return hooks.browse ? hooks.browse(request, result) : result;
      }
      case "read_steam_workshop_installation_status": return { consumer_app_id: appId, searched_roots: ["fixture-cache"],
        items: ids.map((item_id) => ({ item_id, path: `fixture-cache/${item_id}`, installed: state.cached.has(item_id) })) };
      case "read_dontstarve_mod_configuration_specs": return ids.map((mod_id) => ({ mod_id, client_only: false,
        status: state.cached.has(mod_id) ? "loaded" : "missing_mod", options: state.cached.has(mod_id) ? [
          { name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 },
            options: [1, 10].map((value) => ({ label: String(value), value: { kind: "number", value } })) }
        ] : [] }));
      case "download_steam_workshop_items":
        hooks.check(args.instanceId === state.details.summary.id, "Download stays scoped to its instance");
        hooks.download?.([...ids]);
        ids.forEach((id) => state.cached.add(id));
        return { consumer_app_id: appId, install_root: "fixture-instance", workshop_root: "fixture-cache",
          items: ids.map((item_id) => ({ item_id, expected_path: `fixture-cache/${item_id}`, expected_path_exists: true })) };
      case "update_instance_record_if_current": {
        hooks.check(args.expectedSettingsJson === state.details.settings_json, "Save checks the current settings revision");
        const input = args.input as UpdateInstanceInput;
        hooks.check(input.id === state.details.summary.id, "Save stays scoped to its instance");
        const saved = { ...state.details, settings_json: input.settings_json };
        hooks.save(saved);
        return structuredClone(saved);
      }
      default: throw new Error(`Unexpected fixture IPC ${command}`);
    }
  };
}
