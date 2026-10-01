import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { readInstanceDetails, readModuleDetails, updateInstance } from "../../src/api";
import { I18nProvider } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const errors: string[] = [];
const checks: string[] = [];
const shardKeys = ["master", "caves", "islands", "volcano"];
const modId = "1467214795";
let details: InstanceDetails;
let descriptor: ModuleDetails;
let mode: "configuration" | "mods" | "cmd" = "configuration";
let epoch = 0;
let writes = 0;
const logReads: number[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  if (!value) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return value;
}
function settings(): Record<string, unknown> { return JSON.parse(details.settings_json); }
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(label);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  await act(async () => { target.focus(); target.click(); });
}
async function select(target: HTMLSelectElement, value: string) {
  await act(async () => { target.value = value; target.dispatchEvent(new Event("change", { bubbles: true })); });
}
function Harness() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <main className="server-detail-panel" style={{ flex: 1, minHeight: 0 }}>
      {mode === "configuration" ? <ConfigurationWorkspace details={details} moduleDetails={descriptor}
        moduleDetailsError={null} onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }}
        bindAddressCandidates={[]} runtime={null} launchPlan={null} launchPlanError={null} onSave={save} />
        : mode === "mods" ? <ModWorkbench details={details} moduleDetails={descriptor} launchPlan={null} onSaveSettings={save} />
          : <RuntimeSurfaceWorkbench details={details} moduleDetails={descriptor} runtime={null} runtimeWindows={null}
            startupPending={false} launchHostSurface="managed" />}
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() {
  root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Harness key={epoch} /></InstanceSettingsSaveProvider></I18nProvider>);
}
async function save(input: UpdateInstanceInput, options?: { expectedSettingsJson?: string }) {
  const saved = await updateInstance(input, options?.expectedSettingsJson ?? details.settings_json);
  draw();
  return saved;
}
async function mount(nextMode: typeof mode) {
  mode = nextMode;
  epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(nextMode === "configuration" ? ".configuration-workspace"
    : nextMode === "mods" ? ".mw-sort-pill" : ".server-runtime-console-tabs")), "Workspace did not mount");
}
async function section(id: string) {
  for (let count = 0; count < 5; count++) {
    const collapsed = [...fixture.querySelectorAll<HTMLButtonElement>('.configuration-section-navigation button[aria-expanded="false"]')];
    if (!collapsed.length) break;
    await act(async () => { collapsed.forEach((button) => button.click()); });
  }
  await click(element(`[data-configuration-section-id="${id}"] > .configuration-section-navigation__button`));
}
function row(): HTMLElement { return element(".mw-entry-row"); }
function optionControl(): HTMLSelectElement {
  const label = [...fixture.querySelectorAll<HTMLLabelElement>(".dst-mod-spec-copy")].find((node) => node.textContent === "Difficulty");
  if (!label) throw new Error("Missing Difficulty option");
  return element(`#${CSS.escape(label.htmlFor)}`);
}
async function myMods() {
  const tab = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-sort-pill")].find((button) => button.textContent === "My Mods");
  check(tab, "My Mods is available");
  await click(tab);
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Mod options did not load");
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  descriptor = await readModuleDetails("dontstarve");
  const original = await readInstanceDetails("srv-dst-terminal-error");
  const values: Record<string, unknown> = { ...JSON.parse(original.settings_json), shard_layout: "island_adventures",
    enable_caves: false, shared_workshop_mod_ids: modId };
  for (const [index, shard] of shardKeys.entries()) {
    values[`${shard}_enabled_workshop_mod_ids`] = modId;
    values[`${shard}_mod_configuration_options`] = { [modId]: { difficulty: index + 1 } };
  }
  details = { ...original, summary: { ...original.summary, id: "fixture-dst-four-shards", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify(values) };
  const item = () => ({ id: modId, title: "Fixture Island Adventures", status: "resolved", item_kind: "item",
    consumer_app_id: 322330, children: [], tags: [], child_count: 0, detail_url: "https://steamcommunity.com/" });
  let callbackId = 1;
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: {
    transformCallback: () => callbackId++, unregisterCallback: () => {},
    invoke: async (command: string, args: Record<string, unknown>) => {
      switch (command) {
        case "read_instance_details_from_storage": return structuredClone(details);
        case "lookup_steam_workshop_items": return ((args.ids ?? []) as string[]).map(item);
        case "read_steam_workshop_item_details": return item();
        case "search_steam_workshop_items": return { app_id: 322330, browse_kind: "item", items: [item()], page: 1, has_more: false, total_count: 1 };
        case "read_background_jobs": return [];
        case "read_steam_workshop_installation_status": return { consumer_app_id: 322330, searched_roots: ["fixture-cache"], items: [{ item_id: modId, installed: true }] };
        case "read_dontstarve_mod_configuration_specs": return [{ mod_id: modId, status: "loaded", client_only: false,
          options: [{ name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 },
            options: [1, 2, 3, 4, 8, 10].map((value) => ({ label: String(value), value: { kind: "number", value } })) }] }];
        case "update_instance_record_if_current": {
          check(args.expectedSettingsJson === details.settings_json, "Save uses the current settings snapshot");
          const input = args.input as UpdateInstanceInput;
          details = { ...details, settings_json: input.settings_json };
          writes += 1;
          return structuredClone(details);
        }
        case "read_instance_log_document_from_storage": {
          const runId = Number(args.runId ?? args.run_id);
          logReads.push(runId);
          const shard = shardKeys[runId - 101];
          return { source_path: `fixture/${shard}/server_log.txt`, lines: [`Fixture ${shard} process log`],
            total_lines: 1, truncated: false, read_error: null };
        }
        case "plugin:event|listen": return callbackId++;
        case "plugin:event|unlisten": return null;
        default: throw new Error(`Unexpected fixture command: ${command}`);
      }
    }
  } });

  await mount("configuration");
  check(!fixture.querySelector('[data-configuration-section-id="mods"]'), "Configuration keeps Mod management in the Mods workspace");
  await section("cluster-shard-coordination");
  const layout = element<HTMLSelectElement>('[data-field-key="shard_layout"] select');
  check(layout.options.length === 2 && !layout.disabled, "Layout selection is visible and editable");
  await select(layout, "0");
  await settle(() => settings().shard_layout === "standard", "Standard layout did not save");
  await select(element('[data-field-key="shard_layout"] select'), "1");
  await settle(() => settings().shard_layout === "island_adventures", "IA layout did not save");
  await section("advanced");
  for (const shard of ["islands", "volcano"]) check(
    !element<HTMLTextAreaElement>(`[data-field-key="${shard}_worldgenoverride_lua"] textarea`).disabled,
    `${shard} expert world file is available in Advanced`);

  await mount("mods");
  await myMods();
  const shardSelector = element<HTMLSelectElement>(".dst-mod-shard-field select");
  check(Array.from(shardSelector.options, (option) => option.value).join(",") === "all,master,caves,islands,volcano", "Mods exposes all four shard targets");
  await select(shardSelector, "islands");
  check(optionControl().value === "number:3", "Islands displays its independent saved options");
  await select(optionControl(), "number:8");
  await settle(() => (settings().islands_mod_configuration_options as Record<string, Record<string, unknown>>)[modId]?.difficulty === 8, "Islands edit did not save");
  check((settings().volcano_mod_configuration_options as Record<string, Record<string, unknown>>)[modId].difficulty === 4, "Islands edit preserves Volcano options");
  await select(element(".dst-mod-shard-field select"), "all");
  await select(optionControl(), "number:10");
  await settle(() => shardKeys.every((shard) => (settings()[`${shard}_mod_configuration_options`] as Record<string, Record<string, unknown>>)[modId].difficulty === 10), "All-shard options did not save");
  check(true, "All-shard editing synchronizes the selected Mod across four shards");
  await click(row().querySelector<HTMLInputElement>("input")!);
  await settle(() => shardKeys.every((shard) => settings()[`${shard}_enabled_workshop_mod_ids`] === ""), "Four-shard disable did not save");
  await click(row().querySelector<HTMLInputElement>("input")!);
  await settle(() => shardKeys.every((shard) => settings()[`${shard}_enabled_workshop_mod_ids`] === modId), "Four-shard enable did not save");
  check(true, "One Mod toggle updates all four native enable lists");

  const raw = `return { ["workshop-${modId}"] = { enabled = true, configuration_options = { difficulty = 8, nested = { keep = true } } } }`;
  const imported = { ...settings(), islands_modoverrides_lua: raw };
  for (const shard of shardKeys) { imported[`${shard}_enabled_workshop_mod_ids`] = ""; imported[`${shard}_mod_configuration_options`] = {}; }
  details = { ...details, settings_json: JSON.stringify(imported) };
  await mount("mods");
  await myMods();
  check(row().querySelector<HTMLInputElement>("input")?.checked, "Imported Lua remains authoritative and exposes its enabled Mod");
  await select(element(".dst-mod-shard-field select"), "islands");
  check(optionControl().disabled && optionControl().value === "number:8", "Imported literal options appear read-only");
  await click(element(".mw-selected-config-empty-note summary"));
  check(fixture.querySelector("pre")?.textContent === raw, "Original nested source Lua remains intact and visible");

  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 4 },
    active_run: { run_id: 101, pid: 201, log_path: "fixture/master/server_log.txt", started_at: "2026-09-30T00:00:00Z",
      processes: shardKeys.map((shard, index) => ({ run_id: 101 + index, process_key: shard,
        display_name: shard, pid: 201 + index, status: "running", log_path: `fixture/${shard}/server_log.txt`, is_primary: index === 0 })) } };
  await mount("cmd");
  const tabs = [...fixture.querySelectorAll<HTMLButtonElement>('.server-runtime-console-tabs [role="tab"]')];
  check(tabs.length === 4, "CMD renders four process tabs");
  for (let index = 1; index < tabs.length; index++) {
    await click(tabs[index]);
    await settle(() => fixture.textContent?.includes(`Fixture ${shardKeys[index]} process log`) === true, "Selected shard log did not load");
    check(tabs[index].getAttribute("aria-selected") === "true", `${shardKeys[index]} CMD tab selects its own process log`);
  }
  check([102, 103, 104].every((runId) => logReads.includes(runId)), "CMD reads each selected shard's own run ID");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Four-shard workspace fits the desktop viewport");
  check(errors.length === 0, "Browser console and uncaught errors remain empty");
  return { status: "passed", checks, writes, cmd_shards: shardKeys, log_reads: logReads, browser_errors: errors };
}

Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  return { browser_errors: errors, native_dialogs: 0 };
} });
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("DST four-shard browser exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); })
  .then((report) => {
    document.getElementById("report")!.textContent = JSON.stringify(report, null, 2);
    return fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
      { method: "POST", body: JSON.stringify(report) });
  });
