import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, ProjectZomboidWorkshopModsSnapshot,
  SteamWorkshopLookupItem, UpdateInstanceInput } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const X = "111111", Y = "222222", Z = "333333", O = "444444", A = "456789", B = "567890";
const leaves = [X, Y, Z, O];
const games = ["projectzomboid", "palworld", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask", "unturned", "terraria", "squad"];
const listFields: Record<string, string> = { projectzomboid: "workshop_items", palworld: "mod_package_names",
  arksurvivalevolved: "active_mod_ids", barotrauma: "mod_workshop_ids", conanexiles: "mod_workshop_ids",
  soulmask: "mod_workshop_ids", unturned: "workshop_file_ids", terraria: "tmodloader_workshop_item_ids" };
const checks: string[] = [], errors: string[] = [], downloads: string[][] = [], reports: Record<string, unknown>[] = [];
const snapshotRequests: string[][] = [];
const writes: { instance: string; command: string }[] = [];
let stored: InstanceDetails, details: InstanceDetails, moduleDetails: ModuleDetails, catalog: SteamWorkshopLookupItem[];
let inventory: ManualModInventoryResult | null = null, epoch = 0, game = "", pzFresh = false, pzMissingMetadata = false;
const cached = new Set<string>();
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${game}: ${label}: ${fixture.textContent}`);
  checks.push(`${game}: ${label}`);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector);
  if (!found) throw new Error(`${game}: Missing ${selector}: ${fixture.textContent}`);
  return found;
}
function button(label: string, within: ParentNode = fixture): HTMLButtonElement {
  const found = [...within.querySelectorAll<HTMLButtonElement>("button")].find((entry) =>
    entry.textContent?.trim() === label || entry.getAttribute("aria-label") === label);
  if (!found) throw new Error(`${game}: Missing button ${label}: ${fixture.textContent}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${game}: ${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const rect = target.getBoundingClientRect();
  assert(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 &&
    rect.top >= -1 && rect.bottom <= innerHeight + 1, "Action is reachable inside the viewport");
  assert(!((target instanceof HTMLButtonElement || target instanceof HTMLInputElement) && target.disabled), "Action is enabled");
  await act(async () => { target.focus(); target.click(); });
}
function settings(): Record<string, unknown> { return JSON.parse(stored.settings_json); }
function values(key: string): string[] { return String(settings()[key] ?? "").split(/[;\n]+/).map((id) => id.trim()).filter(Boolean); }
function records(): { id: string; member_ids: string[] }[] { return settings().steam_workshop_collections as { id: string; member_ids: string[] }[]; }
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function mount() {
  details = structuredClone(stored); epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
}
function typeButton(label: "Mods" | "Collections") { return button(label, element('[role="group"][aria-label="Workshop content type"]')); }
async function library() {
  await click(typeButton("Collections")); await click(button("My collections"));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-row")), "Owned collections did not load");
}
async function myMods() { await click(typeButton("Mods")); await click(button("My Mods")); }
function collectionRow(id: string): HTMLElement {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-collection-row")].find((row) => row.textContent?.includes(`#${id}`))!;
}
async function expand(id: string) {
  const target = element<HTMLButtonElement>(".mw-collection-select", collectionRow(id));
  if (target.getAttribute("aria-expanded") !== "true") await click(target);
}
function member(id: string, collection = A): HTMLElement {
  return [...element(`#collection-members-${collection}`).querySelectorAll<HTMLElement>(".mw-collection-member-row")]
    .find((row) => row.textContent?.includes(id))!;
}
function toggle(id: string, collection = A): HTMLInputElement { return element<HTMLInputElement>('input[type="checkbox"]', member(id, collection)); }
function whole(id = A): HTMLInputElement { return element<HTMLInputElement>(".mw-collection-enabled-toggle", collectionRow(id)); }
function myRows(id: string): HTMLElement[] {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-list > .mw-entry-row")]
    .filter((row) => game === "squad" ? row.querySelector(".mw-entry-row-title")?.textContent?.trim() === id
      : row.querySelector(".mw-entry-id")?.textContent?.includes(game === "palworld" ? `Package${id}` : id));
}
function owned(id: string): boolean {
  if (game === "squad") return Boolean(inventory?.items.some((item) => item.name === id));
  if (game === "palworld") return !(settings().steam_workshop_removed_mod_ids as string[] | undefined)?.includes(id);
  if (game === "arksurvivalevolved") return values("auto_managed_mod_ids").includes(id);
  return values(listFields[game]).includes(id) ||
    Boolean((settings().steam_workshop_disabled_mod_ids as string[] | undefined)?.includes(id));
}
function enabled(id: string): boolean {
  if (game === "projectzomboid") return values("mods").includes(id === X ? (pzFresh ? "FreshXOnly" : "XOnly")
    : id === Y ? "YOnly" : id === Z ? "ZOnly" : "OutsideOnly");
  return values(listFields[game]).includes(game === "palworld" ? `Package${id}` : id);
}
async function changedOnce(action: () => Promise<void>, ready: () => boolean, label: string) {
  const before = writes.length;
  await action(); await settle(() => writes.length === before + 1 && ready(), label);
  assert(writes.length === before + 1, `${label}: one version-checked save`);
}
function pzSnapshot(ids: string[]): ProjectZomboidWorkshopModsSnapshot {
  return { workshop_root: "fixture-cache", workshop_root_exists: true, items: ids.map((id) => {
    const unique = id === X ? (pzFresh ? "FreshXOnly" : "XOnly") : id === Y ? "YOnly" : id === Z ? "ZOnly" : "OutsideOnly";
    const map = id === X ? (pzFresh ? "FreshXMap" : "XMap") : id === Y ? "YMap" : id === Z ? "ZMap" : "OutsideMap";
    const specs = [{ name: unique, maps: id === X ? [map, "Muldraugh, KY"] : [map] },
      ...(id === X || id === Z ? [{ name: "SharedCore", maps: ["SharedMap"] }] : [])];
    return { workshop_item_id: id, item_path: `fixture-cache/${id}`, status: "installed", mods: (pzMissingMetadata && id === Y ? [] : specs).map((spec) => ({
      directory_name: spec.name, mod_id: spec.name, mod_name: spec.name, mod_path: `fixture-cache/${id}/mods/${spec.name}`,
      status: "loaded", map_ids: spec.maps })) };
  }) };
}
function prepare(template: InstanceDetails, moduleId: string) {
  game = moduleId; pzFresh = false; pzMissingMetadata = game === "projectzomboid"; moduleDetails = buildMockModuleDetails(game);
  assert(moduleDetails.summary.id === game && Boolean(moduleDetails.workshop), "Fixture uses the real module capability definition");
  const appId = moduleDetails.workshop!.consumer_app_id;
  const item = (id: string, title: string): SteamWorkshopLookupItem => ({ ...workshopItem(id, title), consumer_app_id: appId });
  const collection = (id: string, title: string, ids: string[]): SteamWorkshopLookupItem => ({ ...item(id, title),
    item_kind: "collection", child_count: ids.length, children: ids.map((childId) => item(childId, `Member ${childId}`)) });
  catalog = [...leaves.map((id) => item(id, `Member ${id}`)), collection(A, "Primary collection", [X, Y]), collection(B, "Shared collection", [Y, Z])];
  const initial: Record<string, unknown> = { server_name: `Fixture ${game}`, external_setting: "keep",
    fixture_mod_options: { [X]: { difficulty: 10 } }, steam_workshop_collections: [
      { id: A, title: "Primary collection", member_ids: [X, Y] }, { id: B, title: "Shared collection", member_ids: [Y, Z] }] };
  if (listFields[game]) initial[listFields[game]] = leaves.map((id) => game === "palworld" ? `Package${id}` : id).join("\n");
  if (game === "arksurvivalevolved") Object.assign(initial, { auto_managed_mods: true, auto_managed_mod_ids: leaves.join("\n") });
  if (game === "projectzomboid") Object.assign(initial, { mods: "XOnly;SharedCore;YOnly;ZOnly;OutsideOnly",
    map_name: "XMap;SharedMap;YMap;ZMap;OutsideMap;Muldraugh, KY" });
  if (game === "terraria") Object.assign(initial, { server_runtime: "tmodloader", tmodloader_enabled_mod_names: "IndependentRuntimeMod" });
  stored = { ...template, summary: { ...template.summary, id: `fixture-cross-game-${game}`, module_id: game,
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: JSON.stringify(initial) };
  details = structuredClone(stored); cached.clear(); leaves.forEach((id) => cached.add(id));
  const target = `C:\\Fixture\\${game}\\Mods`;
  inventory = moduleDetails.mods?.manual_staging ? { instance_id: stored.summary.id, module_id: game, source_label: "Fixture",
    target_label: "Mods", target_path: target, target_exists: true, items: leaves.map((id) => ({ name: id,
      path: `${target}\\${id}`, item_type: "directory", file_count: 2, total_bytes: 8192,
      inferred_id: game === "palworld" ? `Package${id}` : game === "squad" ? null : id })) } : null;
}
async function runGame(template: InstanceDetails, moduleId: string) {
  prepare(template, moduleId);
  const originalOptions = JSON.stringify(settings().fixture_mod_options), originalCache = JSON.stringify([...cached]);
  const originalInventory = structuredClone(inventory), beforeWrites = writes.length, beforeDownloads = downloads.length;
  const reversible = !["unturned", "terraria", "squad"].includes(game);
  await mount(); await myMods();
  await settle(() => myRows(X).length === 1 && myRows(Y).length === 1, "Collection leaf Mods must appear in My Mods");
  assert(myRows(A).length === 0, "Collection record is not duplicated as a leaf Mod");
  if (!reversible) assert(!myRows(X)[0].querySelector('input[type="checkbox"]'), "My Mods also omits unsupported enable switches");
  if (game === "projectzomboid") {
    await library(); await expand(A);
    await settle(() => !toggle(X).disabled, "Known PZ member metadata did not load");
    const unavailableToggle = member(Y).querySelector<HTMLInputElement>('input[type="checkbox"]');
    assert((!unavailableToggle || unavailableToggle.disabled) && whole().disabled && writes.length === beforeWrites,
      "One owned member with missing metadata blocks whole toggle without hiding the known member control or saving a partial selection");
    pzMissingMetadata = false;
    await mount(); await myMods();
  }
  if (game === "terraria") {
    const configuredRuntime = settings();
    assert(element<HTMLButtonElement>(".mw-entry-remove-button", myRows(X)[0]).disabled && writes.length === beforeWrites,
      "Unknown tModLoader name ownership blocks Workshop removal without changing runtime names");
    await library(); await expand(A);
    await click(element<HTMLButtonElement>(".mw-entry-row-actions > .mw-entry-remove-button", collectionRow(A)));
    await settle(() => Boolean(fixture.querySelector(".mw-collection-removal-dialog[open]")), "Terraria review did not open");
    const review = element<HTMLDialogElement>(".mw-collection-removal-dialog");
    await click(button("Remove collection and selected Mods", review));
    await settle(() => Boolean(review.querySelector('[role="alert"]')), "Unsafe name ownership must produce a visible removal error");
    assert(writes.length === beforeWrites && records().length === 2 && settings().tmodloader_enabled_mod_names === "IndependentRuntimeMod",
      "Unsafe Terraria whole member removal writes nothing and preserves runtime names");
    await changedOnce(() => click(button("Remove collection record only", review)), () => records().length === 1,
      "Terraria can remove only the collection record");
    assert(owned(X) && settings().tmodloader_enabled_mod_names === "IndependentRuntimeMod", "Record-only removal keeps all Terraria Mods and runtime names");
    stored = { ...stored, settings_json: JSON.stringify({ ...configuredRuntime, tmodloader_enabled_mod_names: "" }) };
    await mount(); await myMods();
  }
  await library(); await expand(A); await expand(B);
  await settle(() => Boolean(member(X)?.querySelector(".mw-entry-remove-button")), "Added member removal must be available");
  if (reversible) {
    await settle(() => Boolean(member(X)?.querySelector('input[type="checkbox"]')) && !toggle(X).disabled, "Reversible member control did not load");
    const scanCount = snapshotRequests.length;
    if (game === "projectzomboid") {
      pzFresh = true;
      const latest = settings(); latest.mods = String(latest.mods).replace("XOnly", "FreshXOnly");
      latest.map_name = String(latest.map_name).replace("XMap", "FreshXMap");
      stored = { ...stored, settings_json: JSON.stringify(latest) };
    }
    await changedOnce(() => click(toggle(X)), () => !enabled(X), "Single member disables");
    assert(owned(X) && !toggle(X).checked && whole().indeterminate, "Disabled member remains added and whole state becomes mixed");
    if (game === "projectzomboid") assert(snapshotRequests.slice(scanCount).some((ids) => leaves.every((id) => ids.includes(id))) &&
      !values("map_name").includes("FreshXMap") && values("mods").includes("SharedCore") && values("map_name").includes("SharedMap") &&
      values("map_name").includes("Muldraugh, KY"), "PZ uses a fresh complete scan and preserves shared internal IDs and the vanilla map");
    await myMods(); assert(myRows(X).length === 1 && !element<HTMLInputElement>('input[type="checkbox"]', myRows(X)[0]).checked,
      "Disabled member stays visible and unchecked in My Mods");
    await library(); await expand(A); await expand(B);
    await changedOnce(() => click(whole()), () => enabled(X) && enabled(Y), "Mixed whole control enables all members");
    await changedOnce(() => click(whole()), () => !enabled(X) && !enabled(Y), "Whole control disables its members");
    assert(owned(X) && owned(Y) && enabled(Z) && enabled(O) && !toggle(Y, B).checked, "Whole disable retains ownership and synchronizes shared member state");
    await mount(); await library(); await expand(A); await expand(B);
    assert(!toggle(X).checked && !toggle(Y).checked, "Disabled ownership survives remount");
    await changedOnce(() => click(whole()), () => enabled(X) && enabled(Y), "Whole control restores members without downloading");
    assert(downloads.length === beforeDownloads, "Enablement never downloads cached Mods");
  } else {
    assert(!collectionRow(A).querySelector('input[type="checkbox"]') && !member(X).querySelector('input[type="checkbox"]'),
      "Download-only or file-loaded games do not invent reversible enable switches");
  }
  await changedOnce(() => click(element<HTMLButtonElement>(".mw-entry-remove-button", member(X))), () => !owned(X), "Remove one collection member");
  assert(records().length === 2 && records()[0].member_ids.includes(X), "Single removal retains collection snapshots for repair");
  assert(cached.has(X) && JSON.stringify(settings().fixture_mod_options) === originalOptions, "Single removal retains download cache and saved options");
  if (game === "palworld") assert(!enabled(X) && inventory?.items.some((item) => item.name === X) &&
    (settings().steam_workshop_removed_mod_ids as string[]).includes(X), "Palworld removal stops PackageName and marks retained files as removed");
  await mount(); await myMods();
  await settle(() => myRows(X).length === 0 && myRows(Y).length === 1, "Removed member must stay absent after remount");
  await library(); await expand(A);
  await settle(() => !button("Add missing Mods", collectionRow(A)).disabled, "Missing member repair did not become available");
  if (game === "squad") {
    const beforeRepairWrites = writes.length, beforeRepairDownloads = downloads.length;
    await click(button("Add missing Mods", collectionRow(A)));
    await settle(() => owned(X) && downloads.length === beforeRepairDownloads + 1 && !collectionRow(A).querySelector(".mw-collection-repair-button"),
      "Repair restores the deployed Squad member");
    assert(writes.length === beforeRepairWrites, "Squad file repair does not invent a settings change");
  } else await changedOnce(() => click(button("Add missing Mods", collectionRow(A))), () => owned(X), "Repair restores the removed member");
  assert(downloads.at(-1)?.length === 1 && downloads.at(-1)?.[0] === X, "Repair targets only the missing Workshop member");
  if (game === "palworld") assert(!(settings().steam_workshop_removed_mod_ids as string[] | undefined)?.includes(X) && enabled(X),
    "Palworld restore clears the removal marker and reuses the saved PackageName");
  await click(element<HTMLButtonElement>(".mw-entry-row-actions > .mw-entry-remove-button", collectionRow(A)));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-removal-dialog[open]")), "Whole removal review did not open");
  const dialog = element<HTMLDialogElement>(".mw-collection-removal-dialog");
  const shared = element<HTMLInputElement>(`input[value="${Y}"]`, dialog);
  assert(shared.disabled && !shared.checked && element<HTMLInputElement>(`input[value="${X}"]`, dialog).checked,
    "Whole removal protects members shared with another collection");
  await changedOnce(() => click(button("Remove collection and selected Mods", dialog)), () => records().length === 1 && !owned(X), "Whole collection removal");
  assert(records()[0].id === B && owned(Y) && owned(Z) && owned(O) && settings().external_setting === "keep" &&
    JSON.stringify(settings().fixture_mod_options) === originalOptions && JSON.stringify([...cached]) === originalCache,
    "Whole removal retains other collections, outside Mods, custom settings, options and cache");
  if (game === "projectzomboid") assert(values("mods").includes("SharedCore") && values("map_name").includes("SharedMap") &&
    values("map_name").includes("Muldraugh, KY"), "PZ removal preserves internal IDs needed by remaining Workshop items");
  if (game === "terraria") assert(settings().tmodloader_enabled_mod_names === "", "Workshop ownership does not invent runtime Mod names");
  if (game === "squad") assert(originalInventory?.items.length === 4 && inventory?.items.length === 3,
    "Squad removes only one deployed directory and preserves the other-instance inventory snapshot");
  await mount(); await myMods();
  await settle(() => myRows(X).length === 0 && myRows(Y).length === 1, "Whole removal must survive remount in My Mods");
  await library(); await expand(B);
  assert(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    "Collection layout stays within the viewport");
  reports.push({ module: game, reversible, single_remove_repair: true, protected_whole_removal: true,
    leaf_visibility_and_remount: true, writes: writes.length - beforeWrites, downloads: downloads.length - beforeDownloads });
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const base = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, catalog, cached, inventory }), {
    check: assert, save: (saved) => { stored = structuredClone(saved); details = saved; writes.push({ instance: saved.summary.id, command: "CAS" }); },
    download: (ids) => {
      downloads.push(ids);
      for (const id of ids) if (inventory && !inventory.items.some((item) => item.name === id)) inventory.items.push({ name: id,
        path: `${inventory.target_path}\\${id}`, item_type: "directory", file_count: 2, total_bytes: 8192,
        inferred_id: game === "palworld" ? `Package${id}` : game === "squad" ? null : id });
    }
  });
  mockWindows("cross-game-controls-fixture");
  mockIPC(async (command, payload) => {
    if (command === "read_project_zomboid_workshop_mods_snapshot") {
      const args = payload as { instanceId: string; ids: string[] };
      assert(game === "projectzomboid" && args.instanceId === stored.summary.id, "PZ metadata remains scoped to this instance");
      snapshotRequests.push([...args.ids]); return pzSnapshot(args.ids);
    }
    if (command === "remove_instance_workshop_collection") {
      const args = payload as { input: UpdateInstanceInput; expectedSettingsJson: string; collectionId: string; memberIds: string[]; retainCollection?: boolean };
      assert(game === "squad" && args.input.id === stored.summary.id && inventory?.instance_id === stored.summary.id,
        "Native file removal targets only the Squad instance");
      assert(args.expectedSettingsJson === stored.settings_json, "Native file removal checks the current settings revision");
      const record = records().find((entry) => entry.id === args.collectionId);
      assert(record && args.memberIds.length === 1 && args.memberIds[0] === X && record.member_ids.includes(X), "Native removal targets the selected owned member");
      const next = JSON.parse(args.input.settings_json);
      assert(args.retainCollection === true ? JSON.stringify(next) === JSON.stringify(settings())
        : !next.steam_workshop_collections.some((entry: { id: string }) => entry.id === A),
      "Single native removal retains the collection; whole removal deletes its record");
      inventory = { ...inventory!, items: inventory!.items.filter((item) => !args.memberIds.includes(item.name)) };
      stored = { ...stored, settings_json: args.input.settings_json }; details = structuredClone(stored);
      writes.push({ instance: stored.summary.id, command }); return structuredClone(stored);
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  for (const moduleId of games) await runGame(template, moduleId);
  assert(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, games: reports, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Cross-game controls acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, games: reports, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
