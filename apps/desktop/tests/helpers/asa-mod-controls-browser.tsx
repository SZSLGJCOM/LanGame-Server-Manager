import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US"); document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), moduleDetails = buildMockModuleDetails("arksurvivalascended");
const X = "1346144", Y = "1346145", BOTH = "1346146", LOCAL = "1346147", OTHER = "1346148";
const checks: string[] = [], errors: string[] = [];
let stored: InstanceDetails, details: InstanceDetails, inventory: ManualModInventoryResult, epoch = 0, saves = 0;
let conflictReads = 0, runtimeReads = 0, rawReads = 0, stages = 0, failSave = false, pending: Promise<void> | null = null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`); return found;
}
function button(label: string): HTMLButtonElement {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label);
  if (!found) throw new Error(`Missing button ${label}: ${fixture.textContent}`); return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" }); const rect = target.getBoundingClientRect();
  assert(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 && rect.top >= -1 && rect.bottom <= innerHeight + 1,
    "Action remains reachable inside the viewport");
  assert(!((target instanceof HTMLButtonElement || target instanceof HTMLInputElement) && target.disabled), "Action is enabled");
  await act(async () => { target.focus(); target.click(); });
}
function settings(): Record<string, unknown> { return JSON.parse(stored.settings_json); }
function list(key: string): string[] { const value = settings()[key]; return Array.isArray(value) ? value : String(value ?? "").split(/[\n;,]+/).filter(Boolean); }
function rows(id: string): HTMLElement[] {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-list > .mw-entry-row")].filter((entry) => entry.textContent?.includes(id));
}
function toggle(id: string) { return element<HTMLInputElement>('input[type="checkbox"]', rows(id)[0]); }
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function mount() {
  details = structuredClone(stored); epoch += 1; await act(async () => draw());
  await settle(() => Boolean(fixture.querySelector(".mw-reference-input")), "ASA workbench did not mount");
  await click(button("My Mods"));
  await settle(() => !fixture.textContent?.includes("Reading Mod files"), "Inventory did not finish reading");
}
async function change(id: string, enabled: boolean) {
  const before = saves; await click(toggle(id));
  await settle(() => saves === before + 1 && rows(id).length === 1 && toggle(id).checked === enabled && !toggle(id).disabled, "Toggle did not persist its state");
}
async function remove(id: string) {
  const before = saves; await click(element(".mw-entry-remove-button", rows(id)[0]));
  await settle(() => saves === before + 1 && rows(id).length === 0, "Removed ASA Mod remained visible");
}
async function add(id: string) {
  await click(button("Store"));
  const input = element<HTMLInputElement>(".mw-reference-input");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, id);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const before = saves; await click(button("Enable"));
  await settle(() => saves === before + 1 && input.value === "", "Explicit ASA reference was not saved");
  await click(button("My Mods"));
  await settle(() => rows(X).length === 1 && toggle(X).checked, "Explicit reference did not restore the removed ID");
}
function file(id: string) { return { name: `${id}_Fixture`, inferred_id: id, path: `${inventory.target_path}\\${id}_Fixture`,
  item_type: "directory" as const, file_count: 2, total_bytes: 2048 }; }
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  stored = { ...template, summary: { ...template.summary, id: "fixture-asa-controls", module_id: "arksurvivalascended", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify({ mod_ids_csv: `${X}\n${BOTH}`, passive_mod_ids_csv: `${Y}\n${BOTH}`,
      retained_options: { [X]: { difficulty: 10 } }, unrelated_setting: "keep" }) };
  inventory = { instance_id: stored.summary.id, module_id: stored.summary.module_id, source_label: "CurseForge", target_label: "Mods/83374",
    target_path: "C:\\Fixture\\ASA\\Mods\\83374", target_exists: true, items: [] };
  const base = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, inventory, catalog: [], cached: new Set() }), {
    check: assert, save: (saved) => { stored = saved; details = structuredClone(saved); saves += 1; } });
  mockWindows("main");
  mockIPC(async (command, payload) => {
    if (command === "read_instance_details_from_storage") {
      if (conflictReads && --conflictReads === 0) stored = { ...stored, settings_json: JSON.stringify({ ...settings(), passive_mod_ids_csv: `${Y}\n${BOTH}\n999999` }) };
      if (rawReads && --rawReads === 0) stored = { ...stored, settings_json: JSON.stringify({ ...settings(), custom_launch_flags: '"-passivemods"="1346144"' }) };
      if (runtimeReads && --runtimeReads === 0) return { ...stored, summary: { ...stored.summary, status: "Running", active_process_count: 1 } };
    }
    if (command === "update_instance_record_if_current") {
      if (pending) await pending;
      if (failSave) { failSave = false; throw new Error("Fixture CAS rejected a stale settings revision"); }
    }
    if (command === "resolve_manual_mod_references") {
      const args = payload as { instanceId: string; references: string[] };
      assert(args.instanceId === stored.summary.id, "Reference resolution is scoped to the ASA instance");
      return { instance_id: stored.summary.id, module_id: stored.summary.module_id, source_label: "CurseForge", setting_key: "mod_ids_csv",
        setting_label: "ASA Mod IDs", items: args.references.map((reference) => ({ reference, status: "resolved", resolved_id: reference })), resolved_ids: args.references };
    }
    if (command === "stage_manual_mod_files") {
      stages += 1;
      assert((payload as { instanceId: string }).instanceId === stored.summary.id, "ZIP staging is scoped to the ASA instance");
      return { instance_id: stored.summary.id, module_id: stored.summary.module_id, source_label: "CurseForge", target_label: inventory.target_label,
        target_path: inventory.target_path, affected_root_names: [`${X}_FIXTURE`, "readme.txt"], copied_file_count: 3, copied_total_bytes: 4096,
        items: [{ source_path: "C:\\Fixture\\asa-pack.zip", target_path: inventory.target_path, status: "installed", file_count: 3, total_bytes: 4096 }] };
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount();
  assert(inventory.items.length === 0 && [X, Y, BOTH].every((id) => rows(id).length === 1 && toggle(id).checked), "Native active and passive IDs display once without local files");
  await change(X, false); assert(list("curseforge_disabled_mod_ids").includes(X), "ID-only disable persists ownership");
  await mount(); assert(rows(X).length === 1 && !toggle(X).checked, "ID-only disabled state survives remount"); await change(X, true);
  await change(Y, false); assert(!list("passive_mod_ids_csv").includes(Y), "Passive disable actually stops loading");
  await change(Y, true); assert(list("passive_mod_ids_csv").includes(Y) && !list("mod_ids_csv").includes(Y), "Passive reenable preserves passive mode");
  await change(BOTH, false); assert(!list("mod_ids_csv").includes(BOTH) && !list("passive_mod_ids_csv").includes(BOTH), "Combined mode stops both lists");
  await change(BOTH, true); assert(list("mod_ids_csv").includes(BOTH) && list("passive_mod_ids_csv").includes(BOTH), "Combined mode restores both lists");
  await remove(X); assert(list("curseforge_removed_mod_ids").includes(X), "Remove persists a distinct ownership marker");
  inventory.items = [file(X), file(LOCAL), file(OTHER)];
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), curseforge_removed_mod_ids: [X, OTHER] }) };
  await mount(); await settle(() => rows(LOCAL).length === 1, "Local inventory did not load");
  assert(rows(X).length === 0 && rows(OTHER).length === 0, "Retained cache cannot restore removed entries after remount");
  assert(!toggle(LOCAL).checked, "Unconfigured local inventory is not falsely enabled");
  await change(LOCAL, true); await remove(LOCAL);
  assert(inventory.items.length === 3, "Removing instance ownership preserves every downloaded file");
  await add(`00${X}`); assert(list("mod_ids_csv").includes(X), "Explicit add canonicalizes the numeric ID"); await remove(X);
  await click(button("Store"));
  const transfer = new DataTransfer(), archive = new File(["synthetic fixture"], "asa-pack.zip");
  Object.defineProperty(archive, "path", { value: "C:\\Fixture\\asa-pack.zip" }); transfer.items.add(archive);
  const beforeImport = saves;
  await act(async () => element(".mw-dropzone").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer })));
  await settle(() => saves === beforeImport + 1, "Explicit ZIP import did not restore precise ownership");
  await click(button("My Mods")); await settle(() => rows(X).length === 1, "Imported ID is still hidden");
  assert(!toggle(X).checked && rows(LOCAL).length === 0 && rows(OTHER).length === 0, "ZIP affected roots restore only imported IDs and do not auto-enable them");
  await change(X, true);
  let release!: () => void; pending = new Promise<void>((resolve) => { release = resolve; });
  const beforePending = saves; await click(toggle(X));
  await settle(() => toggle(X).disabled, "Pending operation did not lock controls");
  assert(saves === beforePending && [...fixture.querySelectorAll<HTMLInputElement>(".mw-entry-enabled-toggle")].every((input) => input.disabled), "Pending save blocks duplicate writes");
  await act(async () => { pending = null; release(); });
  await settle(() => saves === beforePending + 1 && !toggle(X).checked && !toggle(X).disabled, "Pending save did not finish");
  const beforeConflict = saves; conflictReads = 2; await click(toggle(X));
  await settle(() => Boolean(fixture.textContent?.includes("Mods changed elsewhere")), "Concurrent membership conflict was not reported");
  assert(saves === beforeConflict && !list("mod_ids_csv").includes(X) && list("passive_mod_ids_csv").includes("999999"), "Concurrent changes are preserved without a stale save");
  await mount(); rawReads = 2; const beforeRaw = saves; await click(toggle(X));
  await settle(() => Boolean(fixture.textContent?.includes("Custom launch flags define Mod loading")), "Final-read raw Mod flags did not block the save");
  assert(saves === beforeRaw && !list("mod_ids_csv").includes(X) && settings().custom_launch_flags === '"-passivemods"="1346144"', "Raw flags arriving before save remain untouched and stop the mutation");
  await mount();
  assert([...fixture.querySelectorAll<HTMLInputElement>(".mw-entry-enabled-toggle")].every((input) => input.disabled) &&
    [...fixture.querySelectorAll<HTMLButtonElement>(".mw-entry-remove-button")].every((input) => input.disabled), "Raw Mod loading visibly disables toggle and remove");
  await click(button("Store")); const beforeRawStage = stages;
  assert(element<HTMLInputElement>(".mw-reference-input").disabled && element(".mw-dropzone").getAttribute("aria-disabled") === "true", "Raw Mod loading disables new references and file imports");
  await act(async () => element(".mw-dropzone").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer })));
  assert(stages === beforeRawStage && saves === beforeRaw, "Disabled raw drop cannot start native file staging");
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), custom_launch_flags: "-NoBattlEye" }) };
  await mount(); runtimeReads = 1; const beforeRunning = saves; await click(toggle(X));
  await settle(() => Boolean(fixture.textContent?.includes("Stop the instance")), "Fresh running state did not stop the mutation");
  assert(saves === beforeRunning, "Running guard writes nothing");
  await mount(); failSave = true; const beforeFailure = saves; await click(toggle(X));
  await settle(() => Boolean(fixture.textContent?.includes("Fixture CAS rejected")), "CAS rejection did not remain visible");
  assert(saves === beforeFailure && !toggle(X).checked, "Failed save cannot falsely enable the Mod");
  await change(X, true); await change(X, false);
  assert(JSON.stringify(settings().retained_options) === JSON.stringify({ [X]: { difficulty: 10 } }) && settings().unrelated_setting === "keep", "All actions preserve options and unrelated settings");
  assert(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, saves, id_only_roundtrip: true, passive_and_combined_modes: true, retained_cache_hidden: true,
    precise_file_restore: true, fresh_state_conflict: true, raw_flags_guard: true, running_guard: true, pending_guard: true, save_failure_preserved: true, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("ASA acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
