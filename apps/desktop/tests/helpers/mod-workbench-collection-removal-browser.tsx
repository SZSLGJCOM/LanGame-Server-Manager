import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [];
const collectionA = "456789", collectionB = "567890";
const memberIds = ["111111", "222222", "333333", "444444"];
const records = [{ id: collectionA, title: "Survival essentials", member_ids: memberIds.slice(0, 3) },
  { id: collectionB, title: "Shared map pack", member_ids: [memberIds[1], memberIds[3]] }];
const catalog = [...memberIds.map((id, index) => workshopItem(id, ["Server controls", "Shared world map", "Optional balance", "Map expansion"][index])),
  ...records.map((record) => ({ ...workshopItem(record.id, record.title), item_kind: "collection" as const,
    child_count: record.member_ids.length, children: record.member_ids.map((id) => ({ id, title: `Member ${id}`,
      item_kind: "item" as const, status: "resolved" as const, consumer_app_id: 322330 })) }))];
const cached = new Set(memberIds);
const downloads: string[][] = [], writes: string[] = [];
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult | null = null;
let epoch = 0, attemptedWrites = 0, failSave = false, holdSave = false;
let releaseSave: (() => void) | null = null;
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
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const rect = target.getBoundingClientRect();
  assert(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 && rect.top >= -1 && rect.bottom <= innerHeight + 1,
    "Removal control is visible inside the viewport");
  assert(!target.matches(":disabled"), "Removal control is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function nativeKey(name: "Tab" | "Escape") {
  await act(async () => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    assert(response.ok, `Native ${name} was dispatched`);
  });
}
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
function settings(): Record<string, unknown> { return JSON.parse(details.settings_json); }
function dialog() { return fixture.querySelector<HTMLDialogElement>(".mw-collection-removal-dialog[open]"); }
function checkbox(id: string) { return element<HTMLInputElement>(`input[type="checkbox"][value="${id}"]`, dialog()!); }
function confirm() { return element<HTMLButtonElement>(".mw-collection-removal-confirm", dialog()!); }
async function mount() {
  epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
  await click(button("Collections", element('[role="group"][aria-label="Workshop content type"]')));
  await click(button("My collections"));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-row")), "Collection library did not load");
}
async function openReview() {
  const trigger = button("Remove collection: Survival essentials");
  await click(trigger);
  await settle(() => Boolean(dialog()), "Removal review did not open");
  return trigger;
}
function collectionRecords() { return settings().steam_workshop_collections as typeof records; }
function viewportFits() {
  const bounds = dialog()!.getBoundingClientRect();
  assert(bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight,
    "Removal dialog fits the desktop viewport");
  assert(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    "Removal dialog does not overflow the workspace");
}
async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const initial = { enable_caves: true, shared_workshop_mod_ids: memberIds.join("\n"),
    master_enabled_workshop_mod_ids: memberIds.join("\n"), caves_enabled_workshop_mod_ids: memberIds.join("\n"),
    master_mod_configuration_options: { [memberIds[0]]: { difficulty: 10 }, [memberIds[1]]: { difficulty: 10 } },
    caves_mod_configuration_options: { [memberIds[0]]: { difficulty: 10 } },
    shared_workshop_collection_ids: `${collectionA}\n${collectionB}`, steam_workshop_collections: records };
  moduleDetails = buildMockModuleDetails("dontstarve");
  details = { ...template, summary: { ...template.summary, id: "fixture-removal-dst", module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify(initial) };
  const unchangedOtherInstance = structuredClone(details);
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, inventory, catalog, cached }), {
    check: assert, save: (saved) => { details = saved; writes.push(saved.summary.id); }, download: (ids) => downloads.push(ids)
  });
  mockWindows("collection-removal-fixture");
  mockIPC(async (command, payload) => {
    if (["update_instance_record_if_current", "remove_instance_workshop_collection"].includes(command)) {
      attemptedWrites += 1;
      if (failSave) throw new Error("Fixture collection removal failed");
      if (holdSave) await new Promise<void>((resolve) => { releaseSave = resolve; });
    }
    if (command === "remove_instance_workshop_collection") {
      const args = payload as { input: UpdateInstanceInput; expectedSettingsJson: string; collectionId: string; memberIds: string[] };
      assert(args.input.id === details.summary.id && inventory?.instance_id === details.summary.id,
        "Squad removal targets only the selected instance inventory");
      assert(args.expectedSettingsJson === details.settings_json, "Squad removal carries the current settings revision");
      const owned = collectionRecords().find((entry) => entry.id === args.collectionId);
      const shared = new Set(collectionRecords().filter((entry) => entry.id !== args.collectionId).flatMap((entry) => entry.member_ids));
      assert(Boolean(owned) && args.memberIds.length > 0 && args.memberIds.every((id) => owned!.member_ids.includes(id) && !shared.has(id)),
        "Squad IPC selection belongs to this collection and excludes shared members");
      assert(JSON.stringify(args.memberIds) === JSON.stringify([memberIds[0]]), "Squad IPC preserves the explicitly unchecked member");
      inventory = { ...inventory!, items: inventory!.items.filter((item) => !args.memberIds.includes(item.name)) };
      details = { ...details, settings_json: args.input.settings_json };
      writes.push(details.summary.id);
      return structuredClone(details);
    }
    return baseIpc(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount();
  const trigger = await openReview();
  assert(dialog()!.matches(":modal") && document.activeElement === button("Cancel", dialog()!),
    "Removal is a native modal with Cancel focused first");
  assert(checkbox(memberIds[0]).checked && checkbox(memberIds[2]).checked && !checkbox(memberIds[1]).checked && checkbox(memberIds[1]).disabled,
    "Only nonshared members start selected and shared members cannot be selected");
  assert(dialog()!.textContent?.includes("Shared") && dialog()!.textContent?.includes("Downloaded files and saved Mod options are kept."),
    "Review explains shared retention and the preserved downloads and options");
  trigger.focus();
  assert(dialog()!.contains(document.activeElement), "Native modal prevents background focus");
  for (let index = 0; index < 7; index += 1) {
    await nativeKey("Tab");
    assert(document.activeElement === document.body || dialog()!.contains(document.activeElement), "Tab does not reach background controls");
  }
  await nativeKey("Escape");
  await settle(() => !dialog(), "Escape did not close idle review");
  assert(document.activeElement === trigger && attemptedWrites === 0 && writes.length === 0,
    "Escape restores the trigger and makes no settings or file writes");

  await openReview();
  await click(checkbox(memberIds[0])); await click(checkbox(memberIds[2]));
  assert(confirm().disabled && !button("Remove collection record only", dialog()!).disabled,
    "Unchecking every removable member keeps record-only removal available");
  await click(checkbox(memberIds[0]));
  details = { ...details, settings_json: JSON.stringify({ ...initial, server_name: "Changed by another editor" }) };
  await click(confirm());
  await settle(() => Boolean(dialog()?.querySelector('[role="alert"]')), "Settings conflict did not stay in the review");
  assert(dialog()!.textContent?.includes("Instance settings changed.") && attemptedWrites === 0 && collectionRecords().length === 2,
    "A changed settings revision rejects the stale review without writing");
  assert(checkbox(memberIds[0]).checked && !checkbox(memberIds[2]).checked, "Conflict preserves the user's reviewed selection");
  await click(button("Cancel", dialog()!));
  details = { ...details, settings_json: JSON.stringify(initial) };
  await mount();
  await openReview();
  await click(checkbox(memberIds[2]));
  failSave = true;
  await click(confirm());
  await settle(() => dialog()?.textContent?.includes("Fixture collection removal failed") === true, "Removal failure did not remain inline");
  assert(writes.length === 0 && collectionRecords().length === 2 && checkbox(memberIds[0]).checked && !checkbox(memberIds[2]).checked,
    "A failed save preserves ownership and the exact member selection");
  failSave = false; holdSave = true;
  const attemptsBeforeRetry = attemptedWrites;
  const submit = confirm();
  await click(submit);
  await settle(() => Boolean(releaseSave) && dialog()?.getAttribute("aria-busy") === "true", "Pending removal did not lock the review");
  assert([...dialog()!.querySelectorAll("button, input")].every((control) => control.matches(":disabled")), "Every review control is disabled while saving");
  await nativeKey("Escape");
  await act(async () => { submit.click(); submit.click(); });
  assert(Boolean(dialog()) && attemptedWrites === attemptsBeforeRetry + 1,
    `Busy Escape and repeated clicks cannot close or submit twice (before ${attemptsBeforeRetry}, after ${attemptedWrites}, dialog ${Boolean(dialog())})`);
  holdSave = false;
  await act(async () => { releaseSave!(); releaseSave = null; });
  await settle(() => !dialog() && collectionRecords().length === 1, "Successful whole-package removal did not close and persist");
  for (const key of ["shared_workshop_mod_ids", "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids"]) {
    assert(JSON.stringify(String(settings()[key]).split(/\D+/).filter(Boolean)) === JSON.stringify(memberIds.slice(1)),
      `${key} removes only the selected nonshared member`);
  }
  assert(collectionRecords()[0].id === collectionB && settings().shared_workshop_collection_ids === collectionB,
    "Whole-package removal keeps the overlapping collection and removes only this native collection ID");
  assert(JSON.stringify(settings().master_mod_configuration_options) === JSON.stringify(initial.master_mod_configuration_options) &&
    JSON.stringify(settings().caves_mod_configuration_options) === JSON.stringify(initial.caves_mod_configuration_options),
    "Removed DST members retain their saved options for a later re-add");
  assert(JSON.stringify([...cached]) === JSON.stringify(memberIds) && downloads.length === 0 && unchangedOtherInstance.settings_json === JSON.stringify(initial),
    "Removal preserves the cache and the other instance's independent settings");
  await mount();
  await click(button("Mods", element('[role="group"][aria-label="Workshop content type"]')));
  assert(![...fixture.querySelectorAll(".mw-entry-row")].some((row) => row.textContent?.includes(memberIds[0])),
    "Removed DST member stays absent from My Mods after remount");
  assert([...fixture.querySelectorAll(".mw-entry-row")].some((row) => row.textContent?.includes(memberIds[1])) &&
    JSON.stringify(settings().master_mod_configuration_options) === JSON.stringify(initial.master_mod_configuration_options),
    "My Mods still shows the shared member while saved options survive removal and remount");

  moduleDetails = buildMockModuleDetails("squad");
  details = { ...details, summary: { ...details.summary, id: "fixture-removal-squad", module_id: "squad" },
    settings_json: JSON.stringify({ server_name: "Squad fixture", steam_workshop_collections: records }) };
  inventory = { instance_id: details.summary.id, module_id: "squad", source_label: "Fixture", target_label: "Mods",
    target_path: "C:\\Fixture\\squad\\Mods", target_exists: true, items: memberIds.map((id) => ({ name: id,
      path: `C:\\Fixture\\squad\\Mods\\${id}`, item_type: "directory", inferred_id: null, file_count: 2, total_bytes: 8192 })) };
  const otherInventory = structuredClone(inventory);
  await mount();
  await openReview();
  await click(checkbox(memberIds[2]));
  failSave = true;
  const squadWritesBefore = writes.length;
  await click(confirm());
  await settle(() => dialog()?.textContent?.includes("Fixture collection removal failed") === true, "Squad removal failure did not remain inline");
  assert(writes.length === squadWritesBefore && inventory.items.length === 4, "Failed Squad removal changes neither settings nor deployed inventory");
  failSave = false;
  await click(confirm());
  await settle(() => !dialog() && collectionRecords().length === 1, "Squad collection removal did not persist");
  assert(JSON.stringify(inventory.items.map((item) => item.name)) === JSON.stringify(memberIds.slice(1)) &&
    JSON.stringify([...cached]) === JSON.stringify(memberIds) && otherInventory.items.length === 4,
    "Squad removes only the selected instance directory and preserves shared, unchecked, cached and other-instance files");
  await mount();
  await click(button("Mods", element('[role="group"][aria-label="Workshop content type"]')));
  await settle(() => fixture.querySelectorAll(".mw-entry-row").length === 3, "Squad inventory did not reload after removal");
  assert(![...fixture.querySelectorAll(".mw-entry-row")].some((row) => row.textContent?.includes(memberIds[0])),
    "Squad My Mods reflects the changed instance inventory after remount");

  // Leave an uncommitted review visible for both viewport screenshots.
  moduleDetails = buildMockModuleDetails("dontstarve"); inventory = null;
  details = { ...unchangedOtherInstance, summary: { ...unchangedOtherInstance.summary, id: "fixture-removal-preview" } };
  await mount(); await openReview(); await click(checkbox(memberIds[2]));
  button("Cancel", dialog()!).focus();
  viewportFits();
  assert(errors.length === 0, "Removal browser has no console or uncaught errors");
  return { status: "passed", checks, browser_errors: errors, cancel_no_writes: true, native_focus: true, shared_protected: true,
    conflict_preserved: true, failure_preserved: true, busy_guarded: true, dst_removal: true, squad_removal: true,
    cache_and_options_retained: true, writes, attempted_writes: attemptedWrites, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Collection removal acceptance exceeded 45 seconds")), 45000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
