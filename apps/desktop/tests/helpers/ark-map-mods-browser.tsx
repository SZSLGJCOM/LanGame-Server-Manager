import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { parseWorkshopIdList } from "../../src/views/settings/guided-setting-values";
import { ASA_MEMBERSHIP_KEYS } from "../../src/views/servers/mod-workbench-asa";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [];
const A = "928708", B = "939713", C = "941450", LOCAL = "1254808";
const MAPS = ["main", "map-island", "map-desert"] as const;
type IniFile = "game_ini" | "game_user_settings_ini";
type ModOverride = { active?: boolean; passive?: boolean; configuration?: Partial<Record<IniFile, string>> | null };
type MapOverride = { mods?: Record<string, ModOverride>; active_order?: string[]; passive_order?: string[] };
let stored: InstanceDetails, details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult;
let epoch = 0, saves = 0, failNextSave = false;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => root.unmount());
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
function button(en: string, cn: string): HTMLButtonElement {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((node) => node.textContent?.trim() === (zh ? cn : en));
  if (!found) throw new Error(`Missing ${en}/${cn}: ${fixture.textContent}`);
  return found;
}
function settings(): Record<string, unknown> { return JSON.parse(stored.settings_json); }
function overrides(): Record<string, MapOverride> { return settings().ark_mod_map_overrides as Record<string, MapOverride> ?? {}; }
function sharedIds(): string[] { return parseWorkshopIdList(settings()[stored.summary.module_id === "arksurvivalascended" ? "mod_ids_csv" : "active_mod_ids"]); }
function active(id: string, map: string): boolean { return overrides()[map]?.mods?.[id]?.active ?? sharedIds().includes(id); }
function configuration(id: string, map: string): Partial<Record<IniFile, string>> | null | undefined { return overrides()[map]?.mods?.[id]?.configuration; }
function allMaps(id: string, expected: boolean): boolean { return MAPS.every((map) => active(id, map) === expected); }
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 7000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  if ((target instanceof HTMLButtonElement || target instanceof HTMLInputElement) && target.disabled) throw new Error(`Disabled action: ${target.outerHTML}`);
  await act(async () => { target.focus(); target.click(); });
}
async function change(target: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    const prototype = target instanceof HTMLSelectElement ? HTMLSelectElement.prototype
      : target instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event(target instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
const rows = (id: string) => [...fixture.querySelectorAll<HTMLElement>(".mw-entry-list > .mw-entry-row")].filter((row) => row.textContent?.includes(id));
const row = (id: string) => { const matches = rows(id); if (matches.length !== 1) throw new Error(`Expected one row for ${id}, found ${matches.length}`); return matches[0]; };
const listToggle = (id: string) => element<HTMLInputElement>(".mw-entry-enabled-toggle", row(id));
const targets = () => element<HTMLElement>("[data-ark-mod-targets]");
type MapSelector = "membership" | "configuration";
const mapTrigger = (kind: MapSelector) => element<HTMLButtonElement>(`[data-ark-mod-map-select="${kind}"]`);
function mapMenu(kind: MapSelector): HTMLElement | null {
  const trigger = mapTrigger(kind), id = trigger.getAttribute("aria-controls");
  return id ? document.getElementById(id) : null;
}
function expanded(kind: MapSelector): boolean { return mapTrigger(kind).getAttribute("aria-expanded") === "true"; }
async function openMapMenu(kind: MapSelector) {
  if (!expanded(kind)) await click(mapTrigger(kind));
  await settle(() => expanded(kind) && Boolean(mapMenu(kind)), `${kind} map dropdown did not open`);
}
async function closeMapMenu(kind: MapSelector) {
  if (expanded(kind)) await click(mapTrigger(kind));
  await settle(() => !expanded(kind), `${kind} map dropdown did not close`);
}
async function key(value: "Enter" | "Tab" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${new URLSearchParams(location.search).get("nonce")}/${value}`, { method: "POST" });
    if (!response.ok) throw new Error(`Native ${value} input failed: ${response.status}`);
  });
}
async function pointerClickAt(x: number, y: number) {
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", body: JSON.stringify({ type: "click", x, y }) });
    if (!response.ok) throw new Error(`Native pointer click failed: ${response.status}`);
  });
}
async function clickMapChoice(choice: HTMLInputElement | HTMLButtonElement) {
  const rect = choice.getBoundingClientRect();
  if (rect.left < 0 || rect.top < 0 || rect.right > innerWidth || rect.bottom > innerHeight || choice.disabled)
    throw new Error(`Map choice is not available for a native click: ${choice.outerHTML} ${JSON.stringify(rect.toJSON())}`);
  await pointerClickAt(rect.x + rect.width / 2, rect.y + rect.height / 2);
}
const target = (key: string) => element<HTMLInputElement>(`input[data-ark-mod-target="${key}"]`, targets());
const sharedMembership = () => element<HTMLInputElement>("[data-ark-mod-membership-shared]");
const sharedConfiguration = () => element<HTMLInputElement>("[data-ark-mod-configuration-shared]");
const configTarget = (key: string) => element<HTMLInputElement>(`input[data-ark-mod-configuration-target="${key}"]`);
const restoreAction = () => element<HTMLButtonElement>("button[data-ark-mod-configuration-inherit]", mapMenu("configuration")!);
const ini = () => element<HTMLTextAreaElement>(".asa-mod-config__input");
const saveIni = () => element<HTMLButtonElement>("[data-asa-mod-save]");
async function selectMod(id: string) {
  await click(element<HTMLButtonElement>(".mw-entry-row-select", row(id)));
  await settle(() => targets().getAttribute("data-ark-mod-targets") === id, `Selected Mod ${id} did not reach its own targets`);
}
async function setMapEnabled(id: string, key: string, enabled: boolean) {
  await selectMod(id);
  const desired = MAPS.filter((map) => map === key ? enabled : active(id, map));
  if (MAPS.every((map) => active(id, map) === desired.includes(map))) return;
  await openMapMenu("membership");
  if (sharedMembership().checked) await saveMembershipChoice(desired.length ? target(desired[0]) : sharedMembership());
  for (const map of MAPS) if (target(map).checked !== desired.includes(map)) await saveMembershipChoice(target(map));
  check(MAPS.every((map) => active(id, map) === desired.includes(map)), "Explicit map choices preserve the effective inherited maps not being changed");
  await closeMapMenu("membership");
}
async function saveMembershipChoice(choice: HTMLInputElement) {
  const before = saves, current = settings();
  const sharedKeys = stored.summary.module_id === "arksurvivalascended" ? ASA_MEMBERSHIP_KEYS : ["active_mod_ids", "auto_managed_mod_ids", "custom_launch_flags"];
  const beforeShared = JSON.stringify(sharedKeys.map((key) => current[key]));
  await clickMapChoice(choice);
  await settle(() => saves === before + 1 && !mapTrigger("membership").disabled, "Current Mod map selection did not save");
  check(JSON.stringify(sharedKeys.map((key) => settings()[key])) === beforeShared,
    "A current-Mod membership or inheritance action preserves shared field values and load order verbatim");
  check(expanded("membership"), "Selecting enabled maps or shared defaults keeps the membership dropdown open");
}
async function useSharedMembership() {
  await openMapMenu("membership");
  if (!sharedMembership().checked) await saveMembershipChoice(sharedMembership());
  check(sharedMembership().checked && MAPS.every((map) => !target(map).checked), "Shared membership excludes all explicit map checkboxes");
  await closeMapMenu("membership");
}
async function setConfigurationTargets(keys: readonly string[] | "shared") {
  const before = stored.settings_json;
  await openMapMenu("configuration");
  if (keys === "shared") {
    if (!sharedConfiguration().checked) await clickMapChoice(sharedConfiguration());
  } else {
    if (sharedConfiguration().checked) await clickMapChoice(keys.length ? configTarget(keys[0]) : sharedConfiguration());
    for (const key of MAPS) if (configTarget(key).checked !== keys.includes(key)) {
      await clickMapChoice(configTarget(key));
      check(expanded("configuration"), "Selecting a parameter destination keeps its checkbox dropdown open");
    }
  }
  check(expanded("configuration") && sharedConfiguration().checked === (keys === "shared")
    && MAPS.every((key) => configTarget(key).checked === (keys !== "shared" && keys.includes(key))),
    "Shared parameters and individual map checkboxes remain mutually exclusive in one open dropdown");
  await closeMapMenu("configuration");
  check(stored.settings_json === before, "Selecting parameter destinations does not change Mod enablement or saved parameters");
}
async function selectFile(file: IniFile) { await change(element<HTMLSelectElement>("[data-asa-mod-file]"), file); }
async function saveText(text: string) {
  await change(ini(), text);
  const before = saves;
  await click(saveIni());
  await settle(() => saves === before + 1 && saveIni().disabled, "Current Mod INI did not persist");
}
function noGlobalMapScope(context: string) {
  check(!fixture.querySelector("[data-ark-mod-scope], [data-ark-mod-list-scope] select"), `${context} has no global map switch`);
}
async function verifyCurrentModLayout(id: string) {
  noGlobalMapScope("My Mods");
  const panel = element(".mw-detail-config-panel"), current = targets();
  check(panel.contains(current) && current.getAttribute("data-ark-mod-targets") === id, "Map enablement is attached to the current Mod in its right-hand panel");
  check(!expanded("membership") && !current.querySelector("input[data-ark-mod-target]"), "Enabled maps use a collapsed dropdown instead of a flat checkbox list");
  check(!current.querySelector("[data-ark-mod-membership-inherit], .ark-mod-maps__inherit")
    && current.querySelectorAll("button").length === 1, "Membership inheritance has no external status or reset row");
  await openMapMenu("membership");
  check(MAPS.every((key) => target(key).labels?.length), "Every map in the dropdown has an accessible checkbox label");
  check(MAPS.every((key) => mapMenu("membership")?.contains(target(key))), "The enabled-map checkboxes belong to their trigger's controlled group");
  await closeMapMenu("membership");
  check(fixture.querySelectorAll("[data-ark-mod-targets]").length === 1, "The workbench exposes one current-Mod map selector");
}
function verifySummary(kind: MapSelector, count: number) {
  const trigger = mapTrigger(kind);
  check(!expanded(kind) && new RegExp(`${count}\\s*/\\s*${MAPS.length}`).test(trigger.textContent ?? ""),
    `The collapsed ${kind} dropdown summarizes ${count} of ${MAPS.length} maps`);
}
function verifySharedMembershipSummary() {
  const trigger = mapTrigger("membership");
  check(!expanded("membership") && trigger.textContent?.includes(zh ? "跟随共用" : "Use shared defaults")
    && !/\d+\s*\/\s*\d+/.test(trigger.textContent ?? ""), "Inherited membership displays shared defaults without an explicit map count");
}
async function verifyUnifiedMembershipSelector() {
  const parameters = () => JSON.stringify([settings().asa_mod_configurations, ...MAPS.map((map) => [configuration(A, map), configuration(B, map)])]);
  const beforeParameters = parameters(), beforeShared = sharedIds().join(), edition = stored.summary.module_id;
  verifySharedMembershipSummary();
  await openMapMenu("membership");
  check(mapMenu("membership")?.querySelector("input[type=checkbox]") === sharedMembership()
    && Boolean(sharedMembership().labels?.length) && sharedMembership().checked && MAPS.every((map) => !target(map).checked),
    `${edition} starts with shared membership selected and explicit map checkboxes empty`);
  await saveMembershipChoice(target("map-island"));
  check(!sharedMembership().checked && target("map-island").checked && !target("main").checked && !target("map-desert").checked
    && active(A, "map-island") && !active(A, "main") && !active(A, "map-desert"), `${edition} selecting one inherited map enables only that map`);
  check(sharedIds().join() === beforeShared && allMaps(B, true) && parameters() === beforeParameters,
    "A's explicit membership changes preserve shared defaults, sibling B, and all Mod parameters");
  await saveMembershipChoice(sharedMembership());
  check(sharedMembership().checked && allMaps(A, true) && MAPS.every((map) => !target(map).checked),
    "Choosing shared membership persists inheritance for only the current Mod");
  await saveMembershipChoice(sharedMembership());
  check(!sharedMembership().checked && MAPS.every((map) => !target(map).checked) && allMaps(A, false)
    && MAPS.every((map) => overrides()[map]?.mods?.[A]?.active === false), "Deselecting shared membership persists explicit off on every map");
  check(allMaps(B, true) && parameters() === beforeParameters && sharedIds().join() === beforeShared,
    "Empty current-Mod membership preserves sibling enablement, shared inventory, and every parameter file");
  await saveMembershipChoice(sharedMembership()); await closeMapMenu("membership");
  const beforeDisable = saves; await click(listToggle(A));
  await settle(() => saves === beforeDisable + 1 && allMaps(A, false), "Shared disable did not propagate to inherited maps");
  check(allMaps(B, true) && parameters() === beforeParameters, "A later shared disable propagates to inherited maps without changing sibling Mods or parameters");
  const beforeEnable = saves; await click(listToggle(A));
  await settle(() => saves === beforeEnable + 1 && allMaps(A, true), "Shared enable did not propagate to inherited maps");
  check(sharedIds().toSorted().join() === beforeShared.split(",").toSorted().join() && allMaps(B, true) && parameters() === beforeParameters,
    "A later shared enable restores inherited maps and preserves sibling Mods and parameters");
  if (sharedIds().join() !== beforeShared) await reorderShared(A, B);
  verifySharedMembershipSummary();
}
async function verifyDropdownInteraction(kind: MapSelector) {
  await closeMapMenu(kind);
  const trigger = mapTrigger(kind);
  check(Boolean(trigger.getAttribute("aria-controls")) && trigger.getAttribute("aria-expanded") === "false",
    `${kind} map trigger exposes its controlled group and collapsed state`);
  await act(async () => trigger.focus());
  await key("Enter");
  await settle(() => expanded(kind), `Enter did not open the ${kind} dropdown`);
  const menu = mapMenu(kind)!;
  check(menu.matches("[data-ark-mod-map-menu]") && menu.getAttribute("role") === "group" && document.activeElement === trigger,
    `Enter opens the ${kind} checkbox group and keeps focus on its trigger`);
  await key("Tab");
  check(document.activeElement === menu.querySelector("input[type=checkbox]"), `Tab enters the first ${kind} map checkbox`);
  await key("Escape");
  await settle(() => !expanded(kind), `Escape did not close the ${kind} dropdown`);
  check(document.activeElement === trigger, `Escape returns focus to the ${kind} map trigger`);
  await openMapMenu(kind);
  await pointerClickAt(2, 2);
  await settle(() => !expanded(kind), `An outside pointer click did not close the ${kind} dropdown`);
  check(document.activeElement !== trigger, `Outside click closes the ${kind} dropdown without stealing focus`);
  await openMapMenu(kind);
  const controls = [...mapMenu(kind)!.querySelectorAll<HTMLInputElement | HTMLButtonElement>("input:not(:disabled), button:not(:disabled)")];
  if (kind === "configuration" && !restoreAction().disabled) {
    await act(async () => controls.at(-2)!.focus());
    await key("Tab");
    check(document.activeElement === restoreAction() && expanded(kind), "Tab reaches the restore action inside the parameter dropdown");
  } else await act(async () => controls.at(-1)!.focus());
  await key("Tab");
  await settle(() => !expanded(kind), `Tab leaving the ${kind} map control did not close its dropdown`);
  check(document.activeElement !== trigger && document.activeElement !== document.body,
    `Tab leaving the ${kind} dropdown preserves the next control's focus`);
}
async function verifyNavigationClosesDropdowns() {
  await openMapMenu("membership");
  await selectMod(B);
  check(!expanded("membership"), "Selecting another Mod closes the enabled-map dropdown");
  await selectMod(A);
  await setConfigurationTargets(["main", "map-island"]);
  await openMapMenu("configuration");
  await selectMod(B); await selectMod(A);
  check(!expanded("configuration"), "Mod navigation closes the parameter-map dropdown without reopening the previous Mod's menu");
  await openMapMenu("configuration");
  await clickMapChoice(sharedConfiguration());
  check(expanded("configuration") && sharedConfiguration().checked && MAPS.every((key) => !configTarget(key).checked),
    "Returning to shared parameters keeps the single dropdown open and clears its map checkboxes");
  await setConfigurationTargets(["main", "map-island"]);
}
async function verifyUnifiedConfigurationSelector() {
  const before = stored.settings_json, trigger = mapTrigger("configuration");
  check(fixture.querySelectorAll('[data-ark-mod-map-select="configuration"]').length === 1
    && !fixture.querySelector("[data-ark-mod-configuration-scope]"),
    "Parameter scope and map destinations use one dropdown without a separate scope select");
  check(trigger.textContent?.includes(zh ? "共用默认参数" : "Shared configuration")
    && !/\d+\s*\/\s*\d+/.test(trigger.textContent ?? ""), "Shared parameter summary displays its name without a map count");
  await openMapMenu("configuration");
  check(restoreAction().disabled && restoreAction().type === "button", "Shared scope exposes a disabled restore action inside its dropdown");
  check(mapMenu("configuration")?.querySelector("input[type=checkbox]") === sharedConfiguration()
    && Boolean(sharedConfiguration().labels?.length) && sharedConfiguration().checked && MAPS.every((key) => !configTarget(key).checked),
    "The first parameter checkbox selects shared defaults and excludes every individual map");
  await clickMapChoice(configTarget("map-island"));
  check(restoreAction().disabled, "Maps without configuration overrides cannot restore shared parameters");
  check(!sharedConfiguration().checked && configTarget("map-island").checked
    && !configTarget("main").checked && !configTarget("map-desert").checked && expanded("configuration"),
    "Choosing one map from shared selects only that map and leaves the dropdown open");
  await clickMapChoice(configTarget("main"));
  check(configTarget("main").checked && configTarget("map-island").checked && !sharedConfiguration().checked && expanded("configuration"),
    "A second parameter-map checkbox adds to the same selection");
  await clickMapChoice(sharedConfiguration());
  check(sharedConfiguration().checked && MAPS.every((key) => !configTarget(key).checked) && expanded("configuration"),
    "Selecting shared defaults clears the UI map selection without closing the dropdown");
  await clickMapChoice(sharedConfiguration());
  check(!sharedConfiguration().checked && MAPS.every((key) => !configTarget(key).checked)
    && saveIni().disabled && restoreAction().disabled && fixture.textContent?.includes(zh ? "请选择要配置参数的地图" : "Select maps to configure"),
    "Deselecting shared defaults permits an empty target selection, disables Save, and explains the next action");
  check(stored.settings_json === before, "Unified parameter-scope choices never persist configuration or change loading maps");
  await closeMapMenu("configuration");
  check(!fixture.querySelector("[data-ark-mod-configuration-inherit]"), "The closed parameter dropdown leaves no external restore row");
  verifySummary("configuration", 0);
  await setConfigurationTargets("shared");
}
async function goMyMods(id?: string) {
  await settle(() => [...fixture.querySelectorAll("button")].some((node) => node.textContent?.trim() === (zh ? "我的 Mod" : "My Mods")),
    "Localized Mod workspace did not finish loading");
  await click(button("My Mods", "我的 Mod"));
  await settle(() => id ? rows(id).length === 1 : Boolean(fixture.querySelector(".mw-config-pane")), "My Mods did not open");
  if (id) await selectMod(id);
}
async function verifySharedEntryModes() {
  const before = stored.settings_json;
  await click(stored.summary.module_id === "arksurvivalascended" ? button("Store", "商店") : button("Popular this week", "本周热门"));
  noGlobalMapScope("Store/import");
  check(!fixture.querySelector("[data-ark-mod-targets], [data-ark-mod-configuration-targets]"), "Store/import actions do not borrow the last Mod's map targets");
  await click(button("Manifest mode", "清单模式"));
  noGlobalMapScope("Manifest");
  check(!fixture.querySelector("[data-ark-mod-targets], [data-ark-mod-configuration-targets]"), "Manifest mode edits shared settings without a map picker");
  check(stored.settings_json === before, "Changing workspace mode does not mutate targets or Mod membership");
}
async function configureEdition(template: InstanceDetails, edition: string, custom?: Record<string, unknown>) {
  moduleDetails = buildMockModuleDetails(edition);
  stored = { ...template, summary: { ...template.summary, id: `fixture-${edition}-mod-targets`, module_id: edition,
    status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify(custom ?? { server_name: "Cluster Main", map_name: edition === "arksurvivalascended" ? "TheCenter_WP" : "TheCenter",
      additional_maps: [{ id: "island", map_name: "TheIsland_WP", name: "Island", enabled: true },
        { id: "desert", map_name: "ScorchedEarth_WP", name: "Desert", enabled: true }],
      ...(edition === "arksurvivalascended" ? { mod_ids_csv: `${A},${B}`, passive_mod_ids_csv: "",
        asa_mod_configurations: { [A]: { game_ini: "[Fixture]\nValue=shared\n", game_user_settings_ini: "[FixtureUser]\nUser=shared\n" },
          [B]: { game_ini: "[Sibling]\nValue=B-shared\n", game_user_settings_ini: "" } } }
        : { active_mod_ids: `${A},${B}`, auto_managed_mod_ids: `${A},${B}` }), unrelated: "retained" }) };
  inventory = { instance_id: stored.summary.id, module_id: edition, source_label: "Fixture", target_label: "Mods",
    target_path: "C:\\Fixture\\Mods", target_exists: true, items: [] };
  details = structuredClone(stored); epoch += 1;
  await act(async () => draw());
  await goMyMods(sharedIds()[0]);
  if (sharedIds().length) await settle(() => !mapTrigger("membership").disabled, "Current Mod map controls did not settle");
}
async function reorderShared(source: string, destination: string) {
  const from = element<HTMLButtonElement>(".mw-entry-row-select", row(source));
  const to = element<HTMLButtonElement>(".mw-entry-row-select", row(destination));
  const before = saves;
  await act(async () => from.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerId: 1 })));
  await act(async () => to.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, button: 0, pointerId: 1 })));
  await act(async () => to.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerId: 1 })));
  await settle(() => saves === before + 1, "Shared Mod load-order change did not save");
}
function finalGeometry(kind: MapSelector) {
  const panel = element(".mw-detail-config-panel"), enabled = targets(), configurationTargets = element("[data-ark-mod-configuration-targets]");
  const footer = element(".shell-activity-bar"), warning = element(".shell-activity-notice");
  const rect = enabled.getBoundingClientRect(), parameterRect = configurationTargets.getBoundingClientRect();
  const panelRect = panel.getBoundingClientRect(), footerRect = footer.getBoundingClientRect(), warningRect = warning.getBoundingClientRect();
  const menuRect = mapMenu(kind)!.getBoundingClientRect();
  check(rect.left >= panelRect.left && rect.right <= panelRect.right && rect.top >= panelRect.top, "Current-Mod map targets remain inside their configuration panel");
  check(rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight, "The compact enabled-map control remains visible in the viewport");
  check(menuRect.left >= panelRect.left && menuRect.right <= panelRect.right && menuRect.top >= panelRect.top && menuRect.bottom <= Math.min(panelRect.bottom, footerRect.top),
    "The open checkbox dropdown fits its scroll panel without covering the activity footer");
  check([...mapMenu(kind)!.querySelectorAll<HTMLInputElement>("input[type=checkbox]")].every((choice) => {
    const option = choice.getBoundingClientRect(); return option.top >= menuRect.top && option.bottom <= menuRect.bottom;
  }), `The open ${kind} dropdown visibly presents every map and shared checkbox`);
  check(parameterRect.left >= 0 && parameterRect.right <= innerWidth, "Parameter target controls fit the viewport");
  check(footerRect.top >= 0 && footerRect.bottom <= innerHeight && warningRect.top >= footerRect.top && warningRect.bottom <= innerHeight,
    "Cross-server compatibility guidance remains visible in the activity footer");
  check(document.documentElement.scrollWidth <= innerWidth && window.scrollY === 0, "The workbench does not overflow or scroll the document horizontally");
  return { current_mod_targets: rect.toJSON(), parameter_targets: parameterRect.toJSON(), open_map_menu: menuRect.toJSON(), panel: panelRect.toJSON(), footer: footerRect.toJSON(), warning: warningRect.toJSON() };
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("main");
  const ipc = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, inventory,
    catalog: [A, B, C, LOCAL].map((id) => ({ ...workshopItem(id, `Fixture ${id}`), consumer_app_id: 346110 })), cached: new Set([A, B, C, LOCAL]) }), {
    check, save: (saved) => {
      if (failNextSave) { failNextSave = false; throw new Error("Controlled parameter restore failure"); }
      stored = saved; details = structuredClone(saved); saves++;
    }
  });
  mockIPC(ipc, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await configureEdition(template, "arksurvivalascended");
  await verifyUnifiedMembershipSelector();
  await verifyUnifiedConfigurationSelector();
  await verifyCurrentModLayout(A);
  verifySharedMembershipSummary();
  await verifyDropdownInteraction("membership");
  check(allMaps(A, true) && !settings().ark_mod_map_overrides, "Existing ASA Mods start shared on all three maps without creating overrides");
  await setMapEnabled(A, "map-desert", false);
  check(active(A, "main") && active(A, "map-island") && !active(A, "map-desert"), "Current Mod A is enabled on exactly two of three maps");
  check(sharedIds().join(",") === `${A},${B}` && allMaps(B, true), "Choosing A's maps preserves shared membership and sibling Mod B on every map");
  verifySummary("membership", 2);
  await selectMod(B);
  verifySharedMembershipSummary();
  check(allMaps(B, true), "Selecting sibling Mod B shows its own three-map shared enablement");
  await selectMod(A); await selectFile("game_ini"); await setConfigurationTargets("shared");
  await change(ini(), "[Fixture]\nValue=shared-draft\n");
  await setConfigurationTargets(["main"]); await change(ini(), "[Fixture]\nValue=main-draft\n");
  const beforeDirtyScopeNavigation = stored.settings_json;
  await setConfigurationTargets("shared");
  check(ini().value === "[Fixture]\nValue=shared-draft\n", "The unified dropdown restores an unsaved shared draft after editing map parameters");
  await setConfigurationTargets([]);
  check(saveIni().disabled, "An empty parameter target selection cannot save a retained draft");
  await setConfigurationTargets(["main"]);
  check(ini().value === "[Fixture]\nValue=main-draft\n" && stored.settings_json === beforeDirtyScopeNavigation,
    "Returning from shared and empty targets restores the map draft without persisting it");
  await selectMod(B); await selectFile("game_ini");
  check(ini().value === "[Sibling]\nValue=B-shared\n", "A's unsaved draft does not leak into sibling Mod B");
  await selectMod(A); await selectFile("game_ini");
  check(ini().value === "[Fixture]\nValue=main-draft\n", "Current-Mod and target-specific drafts survive row navigation");
  await selectFile("game_user_settings_ini");
  check(ini().value === "[FixtureUser]\nUser=shared\n", "Changing the current INI file reads its separate inherited content");
  await selectFile("game_ini"); check(ini().value.includes("main-draft"), "INI file navigation preserves the original draft");
  await setConfigurationTargets("shared"); check(ini().value.includes("shared-draft"), "The shared draft remains separate from map-target drafts");
  await setConfigurationTargets(["main"]); await saveText("[Fixture]\nValue=main-only\n");
  await setConfigurationTargets(["main", "map-island"]);
  verifySummary("configuration", 2);
  await verifyDropdownInteraction("configuration");
  check(Boolean(fixture.querySelector("[data-ark-mod-mixed]")), "Different target values show an explicit mixed state before replacement");
  const beforePairMembership = MAPS.map((key) => active(A, key));
  await saveText("[Fixture]\nValue=two-maps\n");
  check(MAPS.map((key) => active(A, key)).join() === beforePairMembership.join(), "Saving parameters does not change the Mod's selected loading maps");
  for (const key of ["main", "map-island"]) {
    check(configuration(A, key)?.game_ini === "[Fixture]\nValue=two-maps\n", `Current Mod Game.ini saves to selected target ${key}`);
    check(configuration(A, key)?.game_user_settings_ini === undefined, `Saving Game.ini keeps ${key}'s other INI file inherited`);
    check(configuration(B, key) === undefined, `Saving Mod A parameters preserves sibling Mod B on ${key}`);
  }
  check(configuration(A, "map-desert") === undefined && (settings().asa_mod_configurations as Record<string, { game_ini: string }>)[A].game_ini.includes("Value=shared\n"),
    "Multi-map parameter save preserves unselected maps and shared defaults");
  check((settings().asa_mod_configurations as Record<string, { game_ini: string }>)[B].game_ini === "[Sibling]\nValue=B-shared\n",
    "Saving current-Mod parameters preserves sibling Mod B's shared configuration");
  await setConfigurationTargets("shared"); await selectFile("game_user_settings_ini");
  await saveText("[FixtureUser]\nUser=shared-later\n");
  await setConfigurationTargets(["main", "map-island"]);
  await selectFile("game_user_settings_ini");
  check(ini().value === "[FixtureUser]\nUser=shared-later\n" && configuration(A, "main")?.game_user_settings_ini === undefined,
    "A later shared-file change reaches maps that override only the other INI file");
  await selectFile("game_ini");
  check(ini().value === "[Fixture]\nValue=two-maps\n", "Shared-file updates preserve the separately overridden map file");
  await openMapMenu("configuration");
  check(!restoreAction().disabled && mapMenu("configuration")?.contains(restoreAction())
    && restoreAction().getBoundingClientRect().top > configTarget("map-desert").getBoundingClientRect().bottom,
    "The enabled restore action follows the map checkboxes inside the parameter popup");
  const failedRestoreSettings = stored.settings_json, beforeFailedRestore = saves;
  failNextSave = true;
  await act(async () => restoreAction().focus()); await key("Enter");
  await settle(() => fixture.textContent?.includes("Controlled parameter restore failure") === true && !mapTrigger("configuration").disabled,
    "A rejected restore did not expose its error");
  check(stored.settings_json === failedRestoreSettings && saves === beforeFailedRestore && ini().value === "[Fixture]\nValue=two-maps\n"
    && expanded("configuration") && !restoreAction().disabled, "A failed keyboard restore retains saved settings and editor content with a retryable popup action");
  await closeMapMenu("configuration"); await openMapMenu("configuration");
  const beforeParameterInheritance = saves, membershipBeforeReset = MAPS.map((key) => active(A, key));
  await clickMapChoice(restoreAction());
  await settle(() => saves === beforeParameterInheritance + 1 && ini().value === "[Fixture]\nValue=shared\n" && saveIni().disabled,
    "Restoring shared parameters did not refresh the clean editor to the saved shared value");
  check(expanded("configuration") && restoreAction().disabled, "A successful restore keeps the popup open and disables its now-unneeded action");
  check(configuration(A, "main") === undefined && configuration(A, "map-island") === undefined,
    "Parameter inheritance removes only the current Mod's selected-map configuration overrides");
  check(MAPS.map((key) => active(A, key)).join() === membershipBeforeReset.join() && allMaps(B, true)
    && (settings().asa_mod_configurations as Record<string, { game_ini: string }>)[B].game_ini === "[Sibling]\nValue=B-shared\n",
    "Restoring parameters preserves Mod enablement and sibling Mod configuration");
  const afterCleanRestore = stored.settings_json;
  const disabledRestoreRect = restoreAction().getBoundingClientRect();
  await pointerClickAt(disabledRestoreRect.x + disabledRestoreRect.width / 2, disabledRestoreRect.y + disabledRestoreRect.height / 2);
  check(stored.settings_json === afterCleanRestore && saves === beforeParameterInheritance + 1, "Clicking a disabled restore action cannot persist another change");
  await closeMapMenu("configuration");
  check(!fixture.querySelector("[data-ark-mod-configuration-inherit]"), "Closing the popup removes its restore action from the main workspace");
  await saveText("[Fixture]\nValue=two-maps\n");
  check(configuration(A, "main")?.game_ini === "[Fixture]\nValue=two-maps\n"
    && configuration(A, "map-island")?.game_ini === "[Fixture]\nValue=two-maps\n",
    "Editing after parameter inheritance captures the refreshed baseline and persists both target maps");
  await change(ini(), "[Fixture]\nValue=retained-dirty\n");
  await openMapMenu("configuration");
  const beforeDirtyRestore = saves;
  await clickMapChoice(restoreAction());
  await settle(() => saves === beforeDirtyRestore + 1 && configuration(A, "main") === undefined && configuration(A, "map-island") === undefined,
    "Restoring shared parameters with a dirty editor did not persist");
  check(ini().value === "[Fixture]\nValue=retained-dirty\n" && !saveIni().disabled && expanded("configuration")
    && MAPS.map((key) => active(A, key)).join() === membershipBeforeReset.join() && allMaps(B, true)
    && (settings().asa_mod_configurations as Record<string, { game_ini: string }>)[B].game_ini === "[Sibling]\nValue=B-shared\n",
    "Restoring shared parameters preserves the dirty draft, Mod membership, and sibling configuration");
  await closeMapMenu("configuration");
  await click(element<HTMLButtonElement>("[data-asa-mod-reload]")); await click(element<HTMLButtonElement>(".inline-confirm-submit"));
  await settle(() => ini().value === "[Fixture]\nValue=shared\n" && saveIni().disabled, "Confirmed reload did not read the restored shared configuration");
  await saveText("[Fixture]\nValue=two-maps\n");
  check(!stored.settings_json.includes("__ark_mod_editor_baseline"), "The editor's internal baseline never enters persisted instance settings");
  check(Boolean(fixture.querySelector(".shell-activity-notice")), "Different effective map settings display transfer-compatibility guidance");
  await useSharedMembership();
  check(allMaps(A, true), "Current Mod shared enablement restores through its own dropdown");
  check(configuration(A, "main")?.game_ini?.includes("two-maps") && allMaps(B, true),
    "Restoring current-Mod shared enablement preserves its parameter overrides and sibling Mods");
  await setMapEnabled(A, "map-desert", false);
  await verifySharedEntryModes();
  const beforeManifest = saves;
  // The checklist is the authoritative shared list, so adding C means writing the current list plus C.
  const listedBeforeManifest = sharedIds();
  await change(element<HTMLTextAreaElement>(".mw-manifest-input"), [...listedBeforeManifest, C].join("\n"));
  await settle(() => saves === beforeManifest + 1 && sharedIds().includes(C), "Adding shared Mod C through the manifest did not auto-save");
  check(listedBeforeManifest.every((id) => sharedIds().includes(id)), "Rewriting the checklist keeps every listed shared Mod");
  check(allMaps(C, true) && allMaps(B, true) && !active(A, "map-desert"), "A later shared Mod C reaches every map while A's individual exception and B's shared state survive");
  await goMyMods(A); await reorderShared(C, A);
  check(sharedIds()[0] === C && allMaps(C, true) && !active(A, "map-desert"), "Shared load-order edits preserve current-Mod map exceptions");
  const screenshotState = structuredClone(stored), seeded = settings(), seededOverrides = structuredClone(overrides());
  seededOverrides["map-island"] = { ...seededOverrides["map-island"], mods: { ...seededOverrides["map-island"]?.mods, [LOCAL]: { active: true } } };
  await configureEdition(template, "arksurvivalascended", { ...seeded, ark_mod_map_overrides: seededOverrides });
  await selectMod(LOCAL);
  check(!active(LOCAL, "main") && active(LOCAL, "map-island") && !active(LOCAL, "map-desert"), "A map-only Mod appears in the union list without becoming shared");
  verifySummary("membership", 1);
  await setMapEnabled(LOCAL, "map-island", false);
  check(allMaps(LOCAL, false) && !sharedIds().includes(LOCAL), "Disabling a map-only Mod's final target never enables it in shared settings");
  const beforeSibling = saves; await click(listToggle(B));
  await settle(() => saves === beforeSibling + 1, "Shared sibling action did not save");
  check(allMaps(LOCAL, false) && !sharedIds().includes(LOCAL), "An unrelated shared action does not resurrect a disabled map-only Mod from the union list");
  await configureEdition(template, "arksurvivalevolved"); await verifyUnifiedMembershipSelector(); await setMapEnabled(A, "map-desert", false);
  check(active(A, "main") && active(A, "map-island") && !active(A, "map-desert") && allMaps(B, true), "ASE uses the same per-current-Mod two-of-three selection without changing sibling Mods");
  check(parseWorkshopIdList(settings().auto_managed_mod_ids).join() === [A, B].join(), "ASE download intent remains instance-wide");
  await verifyCurrentModLayout(A); await verifySharedEntryModes();
  const empty = { ...settings(), active_mod_ids: "", auto_managed_mod_ids: "", ark_mod_map_overrides: {} };
  await configureEdition(template, "arksurvivalevolved", empty);
  noGlobalMapScope("Empty Mod list");
  check(!fixture.querySelector("[data-ark-mod-targets]"), "An empty Mod list does not expose an unrelated global map selector");
  await configureEdition(template, "arksurvivalascended", JSON.parse(screenshotState.settings_json));
  await selectMod(A); await selectFile("game_ini"); await setConfigurationTargets(["main", "map-island"]);
  await verifyNavigationClosesDropdowns();
  check(ini().value === "[Fixture]\nValue=two-maps\n" && active(A, "main") && active(A, "map-island") && !active(A, "map-desert"),
    "Final screenshot shows the actual current Mod enabled on two maps with its saved two-map parameters");
  await verifyCurrentModLayout(A);
  verifySummary("membership", 2); verifySummary("configuration", 2);
  await act(async () => { fixture.querySelectorAll(".mw-detail-config-panel, .mw-workbench").forEach((node) => { node.scrollTop = 0; });
    window.scrollTo(0, 0); await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  await openMapMenu("membership");
  const membershipGeometry = finalGeometry("membership");
  await closeMapMenu("membership");
  await openMapMenu("configuration");
  const geometry = finalGeometry("configuration");
  await closeMapMenu("configuration");
  if (zh) {
    const beforeScreenshotParameters = MAPS.map((map) => configuration(A, map));
    await useSharedMembership(); verifySharedMembershipSummary();
    check(allMaps(A, true) && JSON.stringify(MAPS.map((map) => configuration(A, map))) === JSON.stringify(beforeScreenshotParameters),
      "The inherited-membership screenshot preserves saved map parameter overrides");
  } else await openMapMenu("configuration");
  check(settings().unrelated === "retained", "Mod operations preserve unrelated instance settings");
  check(errors.length === 0, "Per-Mod map targets produce no browser errors");
  return { status: "passed", checks, final_geometry: geometry, membership_geometry: membershipGeometry, browser_errors: errors, locale: zh ? "zh-CN" : "en-US", native_settings_saved: false };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("ARK per-Mod map acceptance exceeded 50 seconds")), 50000); })])
  .catch((error) => ({ status: "failed", error: String(error), checks, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
