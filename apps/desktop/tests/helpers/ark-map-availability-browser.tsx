import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import type { InstanceDetails, UpdateInstanceInput } from "../../src/types";
import type { InstalledArkMap, InstanceArkMapAvailability } from "../../src/ark-map-availability";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { projectArkModSettings } from "../../src/views/servers/ark-map-mods";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100dvh;display:grid;grid-template-rows:minmax(0,1fr) 40px;padding:16px;gap:12px";
const root = createRoot(fixture);
const store = createActivityNoticeStore();
const descriptor = buildMockModuleDetails("arksurvivalascended");
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [], commands: string[] = [], saves: UpdateInstanceInput[] = [];
const installed: InstalledArkMap = { map_name: "RealAuthorPackage_WP", mod_id: "893657", label: "已下载作者地图" };
let maps: InstalledArkMap[] = [], failRead = false, generation = 0, checks = 0;
let pendingInstance: string | null = null;
let finishPending: ((result: InstanceArkMapAvailability) => void) | null = null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.error instanceof Error ? event.error.stack ?? event.message : event.message));
addEventListener("unhandledrejection", (event) => errors.push(event.reason instanceof Error ? event.reason.stack ?? String(event.reason) : String(event.reason)));
Object.assign(globalThis, { isTauri: true, __reliabilityFixtureCleanup: async () => {
  await act(async () => root.unmount()); return { browser_errors: errors, native_dialogs: 0 };
} });
mockIPC(async (command, args) => {
  commands.push(command);
  if (command !== "read_instance_ark_map_availability") throw new Error(`Unexpected native operation ${command}`);
  const instanceId = String(args.instanceId ?? args.instance_id);
  if (instanceId === pendingInstance) return new Promise<InstanceArkMapAvailability>((resolve) => { finishPending = resolve; });
  if (failRead) throw new Error("Controlled inventory read failure");
  return { instance_id: instanceId, module_id: "arksurvivalascended", maps: structuredClone(maps) };
});

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description); checks++;
}
function node<T extends Element>(selector: string): T {
  const result = fixture.querySelector<T>(selector); if (!result) throw new Error(`Missing ${selector}`); return result;
}
async function settle(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) { await act(async () => target.click()); }
async function change(target: HTMLInputElement | HTMLSelectElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(target instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event(target instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
function initial(instanceId: string, overrides: Record<string, unknown> = {}): InstanceDetails {
  return { summary: { id: instanceId, module_id: "arksurvivalascended", name: "Map acceptance", status: "Stopped", active_process_count: 0,
    autostart: false, bind_ip: "0.0.0.0" }, ports: descriptor.default_ports,
    settings_json: JSON.stringify({ ...buildMockSettingsForModule("arksurvivalascended", "Map acceptance", instanceId),
      map_name: "MissingMain_WP", mod_ids_csv: "100,200", passive_mod_ids_csv: "300,893657,400",
      curseforge_disabled_mod_ids: ["893657"],
      additional_maps: [{ id: "retained", map_name: "MissingAdditional_WP", name: "Retained world", enabled: false }], ...overrides }),
    config_file_path: "C:/synthetic/ark/config/instance.json", saves_path: "C:/synthetic/ark/world",
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5, active_run: null };
}
function Host({ details: initialDetails }: { details: InstanceDetails }) {
  const [details, setDetails] = useState(initialDetails);
  return <ConfigurationWorkspace details={details} moduleDetails={descriptor} moduleDetailsError={null}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null} onSave={async (input, options) => {
      check(options?.expectedSettingsJson === details.settings_json, "Map selection keeps the production CAS baseline");
      saves.push(structuredClone(input));
      const saved = { ...details, settings_json: input.settings_json, ports: input.ports, summary: { ...details.summary, bind_ip: input.bind_ip } };
      setDetails(saved); return saved;
    }} />;
}
async function mount(instanceId: string, overrides: Record<string, unknown> = {}) {
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider key={++generation}>
    <ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: "Close" }}>
    <main className="server-detail-panel" style={{ height: "100%", minHeight: 0 }}>
      <Host details={initial(instanceId, overrides)} />
    </main><footer className="shell-activity-bar"><span className="shell-activity-label">动态</span><ActivityBar store={store} entries={[]} /></footer>
    </ActivityNoticeTarget.Provider></InstanceSettingsSaveProvider></I18nProvider>));
  await settle(() => Boolean(fixture.querySelector('.settings-schema-field--field-map-name input')), "Main map control did not render");
}
const mainInput = () => node<HTMLInputElement>('.settings-schema-field--field-map-name input');
async function openMainChoices() {
  await act(async () => {
    mainInput().dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "mouse" }));
    mainInput().focus();
  });
}
const mainChoices = () => [...fixture.querySelectorAll<HTMLElement>('.settings-schema-field--field-map-name [role="option"]')];
async function section(id: string) {
  for (let depth = 0; depth < 10; depth++) {
    const collapsed = [...fixture.querySelectorAll<HTMLButtonElement>('.configuration-section-navigation button[aria-expanded="false"]')];
    if (!collapsed.length) break;
    await act(async () => collapsed.forEach((button) => button.click()));
  }
  await click(node<HTMLElement>(`[data-configuration-section-id="${id}"] > .configuration-section-navigation__button`));
}
const addPicker = () => node<HTMLSelectElement>('.ark-map-editor__add select');
const addButton = () => node<HTMLButtonElement>('.ark-map-editor__add button');
const missingMap = (map: string) => store.getSnapshot().some((notice) => notice.tone === "warning"
  && React.isValidElement<{ "data-ark-map-unavailable"?: string }>(notice.action) && notice.action.props["data-ark-map-unavailable"] === map);
async function openActivityDetails() {
  if (!fixture.querySelector(".shell-activity-panel")) await click(node<HTMLElement>(".shell-activity-expand"));
}

async function run() {
  await mount("first");
  await settle(() => missingMap("MissingMain_WP"), "Missing primary map warning did not appear");
  check(mainInput().value === "MissingMain_WP", "Inventory read changed the saved primary map");
  await openMainChoices();
  check(mainChoices().length === 10 && !mainChoices().some((choice) => /Atlantis|Club ARK|RealAuthorPackage/.test(choice.textContent ?? "")),
    "The main map initially offers official native maps only");
  await section("transfer");
  check([...addPicker().options].length === 11 && ![...addPicker().options].some((option) => option.value === installed.map_name),
    "The additional map initially offers official native maps and the custom input only");
  check(missingMap("MissingAdditional_WP"), "A retained missing additional world needs a warning");
  check(!fixture.querySelector('.ark-map-editor .shell-activity-notice, .ark-map-editor .ark-editor__error'), "Map warnings still occupy the editor");
  await openActivityDetails();
  await click(node<HTMLElement>('.shell-activity-panel [data-ark-map-unavailable="MissingAdditional_WP"]'));
  check(document.activeElement?.textContent === "MissingAdditional_WP" && !fixture.querySelector(".shell-activity-panel"),
    "Missing-map activity action must focus the retained map and dismiss details");
  check(saves.length === 0, "Reading legacy missing maps must not save or enable Mods");

  maps = [installed];
  await click(node<HTMLElement>('.ark-map-availability button'));
  await settle(() => [...addPicker().options].some((option) => option.value === installed.map_name), "Downloaded map did not appear after reload");
  check([...addPicker().options].find((option) => option.value === installed.map_name)?.textContent?.includes("已下载模组"), "Installed map options identify Mod content");
  await section("room"); await openMainChoices();
  await click(mainChoices().find((choice) => choice.textContent?.includes("已下载作者地图"))!);
  await settle(() => saves.length === 1, "Choosing the primary Mod map did not save");
  let saved = JSON.parse(saves[0].settings_json);
  let selectedMap = projectArkModSettings(saved, "arksurvivalascended", "main");
  check(saved.map_name === installed.map_name && selectedMap.mod_ids_csv === "100\n200\n893657" && selectedMap.passive_mod_ids_csv === "300\n400",
    "The primary map and its active Mod membership must share one saved mutation");
  check(saved.mod_ids_csv === "100,200" && saved.passive_mod_ids_csv === "300,893657,400"
    && !projectArkModSettings(saved, "arksurvivalascended", "map-retained").mod_ids_csv?.toString().includes(installed.mod_id),
    "Choosing the primary map preserves shared defaults and does not activate its Mod on another map");
  check(saved.additional_maps[0].map_name === "MissingAdditional_WP", "Choosing the primary map changed a retained additional world");
  check(!missingMap(installed.map_name), "A discovered selected map is incorrectly marked missing");

  await mount("additional"); await section("transfer");
  await settle(() => [...addPicker().options].some((option) => option.value === installed.map_name), "Additional map query did not complete");
  await change(addPicker(), installed.map_name); await click(addButton());
  await settle(() => saves.length === 2, "Adding a downloaded map did not save");
  saved = JSON.parse(saves[1].settings_json);
  selectedMap = projectArkModSettings(saved, "arksurvivalascended", `map-${saved.additional_maps[1].id}`);
  check(saved.additional_maps.length === 2 && saved.additional_maps[1].map_name === installed.map_name
    && selectedMap.mod_ids_csv === "100\n200\n893657" && selectedMap.passive_mod_ids_csv === "300\n400",
    "Adding a map and enabling its discovered Mod must share one saved mutation");
  check(saved.mod_ids_csv === "100,200" && saved.passive_mod_ids_csv === "300,893657,400"
    && !projectArkModSettings(saved, "arksurvivalascended", "main").mod_ids_csv?.toString().includes(installed.mod_id)
    && !projectArkModSettings(saved, "arksurvivalascended", "map-retained").mod_ids_csv?.toString().includes(installed.mod_id),
    "Adding a map preserves shared defaults and does not activate its Mod on existing maps");
  check(saved.map_name === "MissingMain_WP" && saved.additional_maps[0].map_name === "MissingAdditional_WP", "Adding a map silently replaced a missing saved map");

  await mount("raw", { custom_launch_flags: "-mods=100" });
  await settle(() => fixture.querySelector('.ark-map-availability')?.getAttribute("aria-busy") === "false", "Raw-conflict map query did not complete");
  await openMainChoices(); await click(mainChoices().find((choice) => choice.textContent?.includes("已下载作者地图"))!);
  check(mainInput().value === "MissingMain_WP" && store.getSnapshot().some((notice) => notice.tone === "error" && notice.text.includes("-mods")),
    "A raw Mod flag conflict must block the primary selection with an explanation");
  await section("transfer"); await change(addPicker(), installed.map_name); await click(addButton());
  check(store.getSnapshot().some((notice) => notice.tone === "error" && notice.text.includes("-mods")),
    "A raw Mod flag conflict must block additional map creation with an explanation");
  check(addPicker().getAttribute("aria-invalid") === "true" && !fixture.querySelector('.ark-map-editor > [role="alert"]'),
    "The rejected map selection needs a red frame without an inline error row");
  await openActivityDetails();
  await click(node<HTMLElement>(`.shell-activity-panel [data-configuration-validation-target="${CSS.escape(addPicker().id)}"]`));
  check(document.activeElement === addPicker() && !fixture.querySelector(".shell-activity-panel"), "Map failure action did not focus the rejected selection");
  check(fixture.querySelectorAll('.ark-map-editor tbody tr').length === 2, "Rejected addition leaked a partial map change");

  maps = []; failRead = true;
  await mount("failure");
  await settle(() => store.getSnapshot().some((notice) => notice.text.includes("未能读取已下载的模组地图")), "Read failure was silently treated as empty inventory");
  await openActivityDetails();
  check(fixture.querySelector('.shell-activity-panel .shell-activity-notice-diagnostics')?.textContent?.includes("Controlled inventory read failure"),
    "Read failure details retain the actual cause for troubleshooting");
  check(mainInput().value === "MissingMain_WP" && !missingMap("MissingMain_WP"),
    "An unavailable inventory cannot claim the selected package is missing or replace it");
  failRead = false;
  await click(node<HTMLElement>('.shell-activity-panel .shell-activity-notice.is-error .shell-activity-notice-actions button'));
  await settle(() => fixture.querySelector('.ark-map-availability')?.getAttribute("aria-busy") === "false"
    && !store.getSnapshot().some((notice) => notice.tone === "error"), "Inventory retry did not clear its activity failure");
  check(missingMap("MissingMain_WP"), "A successful retry did not restore accurate missing-map feedback");
  pendingInstance = "stale";
  await mount("stale");
  await settle(() => finishPending !== null, "Deferred old-instance inventory request was not issued");
  await mount("current");
  await settle(() => fixture.querySelector('.ark-map-availability')?.getAttribute("aria-busy") === "false", "Current inventory did not finish");
  await act(async () => { finishPending!({ instance_id: "stale", module_id: "arksurvivalascended", maps: [installed] }); });
  await openMainChoices();
  check(!mainChoices().some((choice) => choice.textContent?.includes("已下载作者地图")), "A late old-instance result leaked into the current picker");
  check(saves.length === 2, "Reads, failed selections or stale results caused a hidden mutation");
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Map availability content overflows the desktop viewport");
  check(commands.every((command) => command === "read_instance_ark_map_availability"), "The map picker performed an installation or startup operation");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, map_saves: saves.length, read_requests: commands.length, browser_errors: errors, installation_executed: false };
}
void run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
