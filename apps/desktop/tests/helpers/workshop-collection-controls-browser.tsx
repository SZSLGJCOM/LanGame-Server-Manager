import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [], downloads: string[][] = [];
const writes: Record<string, unknown>[] = [];
const X = "111111", Y = "222222", Z = "333333", OUTSIDE = "444444", A = "456789", B = "567890";
const cached = new Set<string>();
const moduleDetails = buildMockModuleDetails("dontstarve");
const members = [X, Y, Z, OUTSIDE].map((id) => workshopItem(id, `Member ${id}`));
function collection(id: string, title: string, ids: string[]): SteamWorkshopLookupItem {
  return { ...workshopItem(id, title), item_kind: "collection", child_count: ids.length,
    children: ids.map((childId) => ({ ...workshopItem(childId, `Member ${childId}`) })) };
}
const catalog = [...members, collection(A, "Survival group", [X, Y]), collection(B, "Caves group", [Y, Z])];
let details: InstanceDetails, stored: InstanceDetails, epoch = 0;
let holdSave = false, pendingSaves = 0, releaseSave: (() => void) | undefined;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
function button(label: string, within: ParentNode = fixture): HTMLButtonElement {
  const found = [...within.querySelectorAll<HTMLButtonElement>("button")].find((entry) =>
    entry.textContent?.trim() === label || entry.getAttribute("aria-label") === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 7000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const bounds = target.getBoundingClientRect();
  assert(bounds.width > 0 && bounds.height > 0 && bounds.left >= -1 && bounds.right <= innerWidth + 1 &&
    bounds.top >= -1 && bounds.bottom <= innerHeight + 1, "Control remains visible inside the desktop viewport");
  assert(!((target instanceof HTMLButtonElement || target instanceof HTMLInputElement) && target.disabled), "Control is enabled");
  await act(async () => { target.focus(); target.click(); });
}
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function mount() {
  details = structuredClone(stored); epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
}
function settings(): Record<string, unknown> { return JSON.parse(stored.settings_json); }
function ids(field: string): string[] { return String(settings()[field] ?? "").split(/\D+/).filter(Boolean); }
function enabled(id: string): boolean { return ids("master_enabled_workshop_mod_ids").includes(id) && ids("caves_enabled_workshop_mod_ids").includes(id); }
function options(): string { return JSON.stringify([settings().master_mod_configuration_options, settings().caves_mod_configuration_options]); }
function records(): { id: string; member_ids: string[] }[] { return (settings().steam_workshop_collections ?? []) as { id: string; member_ids: string[] }[]; }
function contentType(label: "Mods" | "Collections") { return button(label, element('[role="group"][aria-label="Workshop content type"]')); }
async function library() {
  await click(contentType("Collections")); await click(button("My collections"));
  await settle(() => Boolean(collectionRow(A) || collectionRow(B)), "Collection library did not load");
}
async function myMods() { await click(contentType("Mods")); await click(button("My Mods")); }
function modRows(id: string): HTMLElement[] {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")].filter((row) => row.querySelector(".mw-entry-id")?.textContent?.includes(id));
}
function modToggle(id: string): HTMLInputElement { return element<HTMLInputElement>('input[type="checkbox"]', modRows(id)[0]); }
function collectionRow(id: string): HTMLElement | undefined {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-collection-row")].find((row) => row.textContent?.includes(`#${id}`));
}
async function expand(id: string) {
  const select = element<HTMLButtonElement>(".mw-collection-select", collectionRow(id));
  if (select.getAttribute("aria-expanded") !== "true") await click(select);
}
function memberRow(collectionId: string, id: string): HTMLElement {
  const region = element(`#collection-members-${collectionId}`);
  const found = [...region.querySelectorAll<HTMLElement>(".mw-collection-member-row")].find((row) => row.textContent?.includes(id));
  if (!found) throw new Error(`Missing member ${collectionId}/${id}`);
  return found;
}
function memberToggle(collectionId: string, id: string): HTMLInputElement { return element<HTMLInputElement>('input[type="checkbox"]', memberRow(collectionId, id)); }
function wholeToggle(id: string): HTMLInputElement { return element<HTMLInputElement>(".mw-collection-enabled-toggle", collectionRow(id)); }
async function changedOnce(action: () => Promise<void>, ready: () => boolean, label: string) {
  const before = writes.length;
  await action(); await settle(() => writes.length === before + 1 && ready(), label);
  assert(writes.length === before + 1, `${label}: exactly one CAS save`);
}
function externalSettings(value: Record<string, unknown>) { stored = { ...stored, settings_json: JSON.stringify(value) }; }

async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  stored = { ...template, summary: { ...template.summary, id: "fixture-collection-controls", module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify({ enable_caves: true, shared_workshop_mod_ids: "", shared_workshop_collection_ids: "",
      master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "", steam_workshop_collections: [] }) };
  details = structuredClone(stored);
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, catalog, cached }), {
    check: assert, save: (saved) => { stored = structuredClone(saved); details = saved; writes.push(JSON.parse(saved.settings_json)); },
    download: (itemIds) => downloads.push(itemIds)
  });
  mockWindows("collection-controls-fixture");
  mockIPC(async (command, payload) => {
    if (command === "update_instance_record_if_current" && holdSave) {
      holdSave = false; pendingSaves += 1;
      await new Promise<void>((resolve) => { releaseSave = resolve; });
      pendingSaves -= 1;
    }
    return baseIpc(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount(); await click(contentType("Collections"));
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 2, "Collection catalog did not load");
  const card = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")].find((row) => row.textContent?.includes("Survival group"))!;
  await click(element<HTMLButtonElement>(".mw-mod-quick-action", card));
  await settle(() => !element<HTMLButtonElement>(".mw-mod-quick-action", card).getAttribute("aria-label")?.startsWith("View details"), "Collection details did not resolve");
  await changedOnce(() => click(element<HTMLButtonElement>(".mw-mod-quick-action", card)), () => records().some((record) => record.id === A), "Empty instance adds collection");
  await myMods(); await settle(() => modRows(X).length === 1 && modRows(Y).length === 1, "Installed leaf Mods did not appear once");
  assert(modRows(A).length === 0 && enabled(X) && enabled(Y), "My Mods contains enabled leaf Mods without a duplicate collection entry");

  const configured = settings();
  for (const key of ["shared_workshop_mod_ids", "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids"]) configured[key] = `${configured[key]}\n${OUTSIDE}`;
  configured.master_mod_configuration_options = { [X]: { difficulty: 10 }, [Y]: { difficulty: 10 } };
  configured.caves_mod_configuration_options = { [X]: { difficulty: 10 }, [Y]: { difficulty: 10 } };
  configured.external_fixture_setting = "preserve"; cached.add(OUTSIDE); externalSettings(configured);
  await mount(); await click(button("Manifest mode"));
  const input = element<HTMLTextAreaElement>(".mw-manifest-input");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, B);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await click(button("Check list"));
  await settle(() => [...fixture.querySelectorAll<HTMLButtonElement>(".mw-manifest-actions button")]
    .some((entry) => entry.textContent?.trim() === "Download missing (1)" && !entry.disabled), "Manifest did not become actionable");
  await changedOnce(() => click(button("Download missing (1)")), () => records().some((record) => record.id === B), "Download-only collection ownership persists");
  await myMods(); await settle(() => modRows(Z).length === 1, "Download-only leaf Mod is missing from My Mods");
  assert(!modToggle(Z).checked && cached.has(Z) && enabled(Y), "Download-only deploys new member disabled and preserves already-enabled shared member");
  assert([X, Y, Z, OUTSIDE].every((id) => modRows(id).length === 1), "Overlapping collections deduplicate all leaf rows");
  const preservedOptions = options(), noDownload = downloads.length;

  await library(); await expand(A); await expand(B);
  await changedOnce(() => click(memberToggle(A, Y)), () => !enabled(Y), "Member off from first collection");
  assert(!memberToggle(B, Y).checked && wholeToggle(A).indeterminate, "Shared member and mixed collection state synchronize");
  await myMods(); assert(!modToggle(Y).checked, "My Mods reflects collection member disablement");
  await library(); await expand(A); await expand(B);
  await changedOnce(() => click(memberToggle(B, Y)), () => enabled(Y), "Member on from overlapping collection");
  assert(memberToggle(A, Y).checked, "First collection reflects shared member enablement");
  await changedOnce(() => click(memberToggle(A, X)), () => !enabled(X), "Prepare mixed collection");
  assert(wholeToggle(A).indeterminate && wholeToggle(A).getAttribute("aria-checked") === "mixed", "Mixed collection has accessible indeterminate state");
  await changedOnce(() => click(wholeToggle(A)), () => enabled(X) && enabled(Y), "Mixed collection enables all members atomically");
  await changedOnce(() => click(wholeToggle(A)), () => !enabled(X) && !enabled(Y), "Whole collection disables all members atomically");
  assert(enabled(OUTSIDE) && settings().external_fixture_setting === "preserve" && options() === preservedOptions && downloads.length === noDownload,
    "Enablement preserves external IDs, custom settings, Mod options and downloads");
  await changedOnce(() => click(wholeToggle(A)), () => enabled(X) && enabled(Y), "Whole collection re-enables its members");

  holdSave = true;
  const beforeBusy = writes.length;
  await click(memberToggle(A, Y)); await settle(() => pendingSaves === 1, "Save did not become pending");
  assert(wholeToggle(A).disabled && memberToggle(B, Y).disabled && element<HTMLButtonElement>(".mw-entry-remove-button", memberRow(A, X)).disabled,
    "Pending save blocks overlapping collection mutations");
  await act(async () => { releaseSave?.(); });
  await settle(() => writes.length === beforeBusy + 1 && !enabled(Y), "Pending save did not complete once");
  await changedOnce(() => click(memberToggle(A, Y)), () => enabled(Y), "Restore shared member after pending save");

  await changedOnce(() => click(element<HTMLButtonElement>(".mw-entry-remove-button", memberRow(A, X))), () => !ids("shared_workshop_mod_ids").includes(X), "Remove single collection member from instance");
  assert(!memberRow(A, X).querySelector('input[type="checkbox"]') && cached.has(X) && options() === preservedOptions,
    "Removed member is missing from collection while cache and options remain");
  await myMods(); assert(modRows(X).length === 0 && modRows(Y).length === 1, "Removed member disappears from My Mods without removing shared member");
  await mount(); await myMods(); assert(modRows(X).length === 0, "Member removal survives remount despite preserved options");
  await library(); await expand(A);
  await changedOnce(() => click(button("Add missing Mods", collectionRow(A))), () => enabled(X), "Repair missing member");
  assert(downloads.at(-1)?.length === 1 && downloads.at(-1)?.[0] === X && options() === preservedOptions, "Repair targets only the missing member and keeps its options");

  const stopped = stored;
  for (const mode of ["running", "raw"] as const) {
    stored = mode === "running" ? { ...stopped, summary: { ...stopped.summary, status: "Running", active_process_count: 1 } }
      : { ...stopped, settings_json: JSON.stringify({ ...JSON.parse(stopped.settings_json), master_modoverrides_lua: 'return { ["workshop-111111"] = { enabled = true } }' }) };
    await mount(); await library(); await expand(A);
    assert(wholeToggle(A).disabled && memberToggle(A, X).disabled && element<HTMLButtonElement>(".mw-entry-remove-button", memberRow(A, X)).disabled,
      `${mode} blocks collection enablement and member removal`);
  }
  stored = stopped; await mount(); await library(); await expand(A); await expand(B);
  const beforeRemove = downloads.length;
  await click(element<HTMLButtonElement>('.mw-entry-row-actions > .mw-entry-remove-button', collectionRow(A)));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-removal-dialog[open]")), "Whole removal review did not open");
  const dialog = element<HTMLDialogElement>(".mw-collection-removal-dialog");
  const removable = element<HTMLInputElement>(`input[value="${X}"]`, dialog), shared = element<HTMLInputElement>(`input[value="${Y}"]`, dialog);
  assert(removable.checked && !removable.disabled && !shared.checked && shared.disabled, "Whole removal defaults to all nonshared members and protects overlap");
  await changedOnce(() => click(button("Remove collection and selected Mods", dialog)), () => !records().some((record) => record.id === A), "Whole collection removal commits once");
  assert(records().some((record) => record.id === B) && enabled(Y) && enabled(OUTSIDE) && !ids("shared_workshop_mod_ids").includes(X) && cached.has(X) &&
    options() === preservedOptions && downloads.length === beforeRemove, "Whole removal retains shared Mods, external settings, cache and options");
  await mount(); await myMods();
  assert(modRows(X).length === 0 && [Y, Z, OUTSIDE].every((id) => modRows(id).length === 1) && !modToggle(Z).checked,
    "Committed removal and download-only disabled member survive remount");
  await library(); await expand(B);
  assert(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1, "Collection controls fit the viewport");
  assert(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, browser_errors: errors, leaf_deduplication: true, download_only_owned_disabled: true,
    shared_member_sync: true, whole_toggle_one_cas: true, member_remove_repair: true, protected_whole_removal: true,
    remount_preserved: true, running_raw_busy_blocked: true, writes: writes.length, downloads, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Collection controls acceptance exceeded 60 seconds")), 60000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
