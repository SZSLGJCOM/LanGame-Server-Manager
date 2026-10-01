import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { readInstanceDetails, readModuleDetails, updateInstance } from "../../src/api";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

// Only the IPC boundary is substituted. All workspace controls and save planning
// run in a real browser against disposable memory, never a native game instance.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const checks: string[] = [];
const id = "123456";
const invalidId = "234567";
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
let epoch = 0;
let offline = false;
let rateLimited = false;
let failSave = false;
let writes = 0;
let downloads = 0;

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function row(modId: string): HTMLElement | undefined {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")]
    .find((entry) => entry.querySelector(".mw-entry-id")?.textContent?.includes(modId));
}
function settings(): Record<string, unknown> { return JSON.parse(details.settings_json); }
function setSettings(value: Record<string, unknown>) { details = { ...details, settings_json: JSON.stringify(value) }; }
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const box = target.getBoundingClientRect();
  assert(box.width > 0 && box.height > 0 && box.left >= 0 && box.right <= innerWidth + 1 &&
    box.top >= 0 && box.bottom <= innerHeight + 1,
    "Interaction target has visible desktop geometry");
  await act(async () => { target.focus(); target.click(); });
}
async function select(target: HTMLSelectElement, value: string) {
  await act(async () => { target.value = value; target.dispatchEvent(new Event("change", { bubbles: true })); });
}
function Harness() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div style={{ flex: 1, minHeight: 0 }}>
      <ModWorkbench key={`${epoch}:${details.summary.id}`} details={details} moduleDetails={moduleDetails}
        launchPlan={null} onSaveSettings={async (input, options) => {
          const saved = await updateInstance(input, options?.expectedSettingsJson ?? "");
          draw();
          return saved;
        }} />
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() {
  root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}>
      <Harness />
    </InstanceSettingsSaveProvider></I18nProvider>);
}
async function render(remount = false) {
  if (remount) epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workspace did not mount");
}
async function myMods() {
  const button = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-sort-pill")]
    .find((target) => target.textContent === "My Mods");
  if (!button) throw new Error("My Mods tab missing");
  await click(button);
}
function optionsControl(name: string): HTMLSelectElement {
  const label = [...fixture.querySelectorAll<HTMLLabelElement>(".dst-mod-spec-copy")]
    .find((entry) => entry.textContent === name);
  if (!label) throw new Error(`Missing option ${name}`);
  return element<HTMLSelectElement>(`#${CSS.escape(label.htmlFor)}`);
}
function baseSettings() {
  return { enable_caves: true, shared_workshop_mod_ids: id, master_enabled_workshop_mod_ids: id,
    caves_enabled_workshop_mod_ids: id, master_mod_configuration_options: { [id]: { difficulty: 10 } } };
}

async function run() {
  moduleDetails = await readModuleDetails("dontstarve");
  const original = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...original, summary: { ...original.summary, id: "fixture-mods", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: "{}" };
  setSettings({ ...baseSettings(), shared_workshop_mod_ids: `${id}\n${invalidId}`,
    master_enabled_workshop_mod_ids: `${id}\n${invalidId}` });
  const limitMessage = `Steam returned HTTP 429. ${"Workshop type verification is temporarily rate-limited. ".repeat(20)}RATE_LIMIT_END`;
  const item = (modId: string) => ({ id: modId, title: modId === id ? "Owned server Mod" : "Steam guide",
    status: modId === invalidId ? "unsupported" : rateLimited ? "unverified" : "resolved",
    item_kind: modId === invalidId ? "guide" : rateLimited ? "unknown" : "item", message: rateLimited ? limitMessage : null,
    consumer_app_id: 322330, children: [], tags: [], child_count: 0, detail_url: "https://steamcommunity.com/" });
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
    const ids = (args?.ids ?? []) as string[];
    switch (command) {
      case "read_instance_details_from_storage": return structuredClone(details);
      case "lookup_steam_workshop_items":
        if (offline) throw new Error("Fixture: Workshop is offline");
        return ids.map(item);
      case "read_steam_workshop_item_details":
        if (offline) throw new Error("Fixture: Workshop is offline");
        return item(String(args.id));
      case "search_steam_workshop_items":
        if (offline) throw new Error("Fixture: Workshop is offline");
        return { app_id: 322330, browse_kind: args.browseKind ?? "item", items: [item(id)], page: 1, has_more: false, total_count: 1 };
      case "read_background_jobs": return [];
      case "read_steam_workshop_installation_status": return { consumer_app_id: 322330, searched_roots: ["fixture-cache"],
        items: [id, invalidId, "999999"].map((item_id) => ({ item_id, installed: true })) };
      case "read_dontstarve_mod_configuration_specs": return ids.map((mod_id) => ({
        mod_id, client_only: false, mod_dir: `fixture/${mod_id}`, modinfo_path: `fixture/${mod_id}/modinfo.lua`,
        status: "loaded", options: [
          { name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 },
            options: [1, 10].map((value) => ({ label: String(value), value: { kind: "number", value } })) },
          { name: "other", label: "Other", default_value: { kind: "boolean", value: false }, options: [] }
        ] }));
      case "update_instance_record_if_current": {
        if (failSave) throw new Error("Fixture save failed");
        assert(args.expectedSettingsJson === details.settings_json, "Save uses the current settings precondition");
        const input = args.input as UpdateInstanceInput;
        assert(input.id === "fixture-mods", "Save remains scoped to the selected fixture instance");
        details = { ...details, settings_json: input.settings_json };
        writes += 1;
        return structuredClone(details);
      }
      case "download_steam_workshop_items": downloads += 1; throw new Error("Unexpected download in local enablement");
      default: throw new Error(`Unexpected fixture command: ${command}`);
    }
  } } });
  await render();
  await myMods();
  await settle(() => row(invalidId)?.textContent?.includes("Steam guide") === true, "Invalid metadata not loaded");
  await click(row(invalidId)!.querySelector<HTMLButtonElement>(".mw-entry-remove-button")!);
  await settle(() => !row(invalidId), "Guide was not removed");
  assert(!String(settings().shared_workshop_mod_ids).includes(invalidId), "Invalid Workshop entry is removed from saved settings");
  assert(!row("999999"), "Machine-only Workshop cache does not appear in My Mods");
  await click(row(id)!.querySelector<HTMLInputElement>("input")!);
  await settle(() => row(id)?.querySelector<HTMLInputElement>("input")?.checked === false, "Disabled entry disappeared");
  assert(Boolean(row(id)), "Disabled owned Mod remains available in My Mods");
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Disabled Mod options did not load");
  assert(fixture.textContent?.includes("Existing shard values differ"), "Implicit-default shard difference is warned before editing");
  await select(optionsControl("Other"), "boolean:true");
  await settle(() => Boolean((settings().caves_mod_configuration_options as Record<string, Record<string, unknown>>)?.[id]?.other),
    "Disabled Mod option change was not saved");
  assert(!(row(id)!.querySelector<HTMLInputElement>("input")!.checked), "Editing options does not enable the Mod");
  const beforeLimit = element(".mw-detail-config-col").getBoundingClientRect();
  rateLimited = true;
  await render(true);
  await myMods();
  await settle(() => [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice-text")]
    .some((notice) => notice.title.includes("RATE_LIMIT_END")), "Rate-limit warning did not reach the activity bar");
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Unverified remote metadata hid local Mod configuration");
  assert(!element(".mw-workbench").querySelector(".shell-activity-notice"), "Workshop warnings occupy no space above the workspace");
  assert(row(id)?.textContent?.includes("Owned server Mod"), "Rate-limited metadata retains the known Mod title");
  assert(!element(".mw-detail-config-col").textContent?.includes("non-installable"), "Unknown remote type is not mislabeled as non-installable");
  const afterLimit = element(".mw-detail-config-col").getBoundingClientRect();
  assert(["x", "y", "width", "height"].every((key) =>
    Math.abs(beforeLimit[key as keyof DOMRect] as number - (afterLimit[key as keyof DOMRect] as number)) < 1),
  "Rate-limit warning does not resize or move the local configuration panel");
  rateLimited = false;
  const retry = [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice")]
    .find((notice) => notice.textContent?.includes("RATE_LIMIT_END"))!.querySelector<HTMLButtonElement>(".shell-activity-notice-actions button")!;
  await click(retry);
  await settle(() => !fixture.textContent?.includes("RATE_LIMIT_END"), "Successful metadata retry did not clear the rate-limit warning");
  offline = true;
  await render(true);
  await myMods();
  await settle(() => Boolean(row(id)), "Saved disabled entry did not survive remount");
  const before = writes;
  await click(row(id)!.querySelector<HTMLInputElement>("input")!);
  await settle(() => row(id)?.querySelector<HTMLInputElement>("input")?.checked === true, "Offline enablement did not complete");
  assert(writes === before + 1 && downloads === 0, "Offline reenabling saves intent without downloading");
  assert((settings().master_mod_configuration_options as Record<string, Record<string, unknown>>)[id].other === true,
    "Offline reenabling preserves previously saved options");
  await click(row(id)!.querySelector<HTMLInputElement>("input")!);
  await settle(() => row(id)?.querySelector<HTMLInputElement>("input")?.checked === false, "Second disable did not complete");
  failSave = true;
  await click(row(id)!.querySelector<HTMLInputElement>("input")!);
  await settle(() => fixture.textContent?.includes("Fixture save failed") === true, "Save failure was not surfaced");
  assert(row(id)?.querySelector<HTMLInputElement>("input")?.checked === false, "Failed reenable keeps the saved disabled state");
  failSave = false;
  await render(true);
  await myMods();
  const optionsBeforeRemoval = ["master", "caves"].map((shard) => settings()[`${shard}_mod_configuration_options`]);
  await click(row(id)!.querySelector<HTMLButtonElement>(".mw-entry-remove-button")!);
  await settle(() => !row(id), "Disabled owned Mod could not be removed");
  assert(!String(settings().shared_workshop_mod_ids).includes(id), "Disabled Mod removal clears its download ownership");
  assert(["master", "caves"].every((shard, index) =>
    JSON.stringify(settings()[`${shard}_mod_configuration_options`]) === JSON.stringify(optionsBeforeRemoval[index])),
  "Removal preserves both shards' saved options for later repair");
  assert((settings().dst_removed_workshop_mod_ids as string[]).includes(id), "Removal records the Mod as absent despite its saved options");
  await render(true);
  await myMods();
  assert(!row(id), "Retained options do not recreate a removed Mod after remount");
  setSettings({ ...baseSettings(), master_modoverrides_lua: "return build_mods()" });
  await render(true);
  await myMods();
  assert([...fixture.querySelectorAll<HTMLInputElement>(".mw-entry-enabled-toggle")].every((control) => control.disabled),
    "Custom raw Lua keeps structured enablement read-only");
  setSettings(baseSettings());
  details = { ...details, summary: { ...details.summary, status: "Running" } };
  await render(true);
  await myMods();
  assert(element<HTMLInputElement>(".mw-entry-enabled-toggle").disabled, "Running instance prevents Mod changes");
  details = { ...details, summary: { ...details.summary, status: "Stopped" } };
  const unrelatedId = "345678";
  const caveOnlySettings = {
    ...baseSettings(), master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "",
    master_mod_configuration_options: { [unrelatedId]: { other: true } },
    caves_mod_configuration_options: { [id]: { difficulty: 10 }, [unrelatedId]: { other: false } }
  };
  setSettings(caveOnlySettings);
  await render(true);
  await myMods();
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Cave-only configuration did not render");
  assert(optionsControl("Difficulty").value === "number:1", "Both shards display the overworld default instead of the caves override");
  await select(element<HTMLSelectElement>(".dst-mod-shard-field select"), "caves");
  assert(optionsControl("Difficulty").value === "number:10", "Caves-only editing displays its own explicit value");
  await select(optionsControl("Other"), "boolean:true");
  await settle(() => Boolean((settings().caves_mod_configuration_options as Record<string, Record<string, unknown>>)[id]?.other),
    "Caves-only edit was not saved");
  assert(!(settings().master_mod_configuration_options as Record<string, unknown>)[id],
    "Caves-only editing preserves the overworld implicit defaults");
  await select(element<HTMLSelectElement>(".dst-mod-shard-field select"), "master");
  assert(optionsControl("Difficulty").value === "number:1", "Master-only editing displays its own implicit default");
  await select(element<HTMLSelectElement>(".dst-mod-shard-field select"), "all");
  await select(optionsControl("Other"), "boolean:true");
  await settle(() => Boolean((settings().master_mod_configuration_options as Record<string, Record<string, unknown>>)[id]?.other),
    "Both-shard edit was not saved");
  for (const shard of ["master", "caves"]) {
    const configuration = settings()[`${shard}_mod_configuration_options`] as Record<string, Record<string, unknown>>;
    assert(configuration[id].other === true && !Object.prototype.hasOwnProperty.call(configuration[id], "difficulty"),
      `${shard} saves the displayed overworld baseline when both shards synchronize`);
    assert(configuration[unrelatedId].other === (shard === "master"), `${shard} synchronization preserves another Mod's configuration`);
  }
  setSettings(caveOnlySettings);
  await render(true);
  await myMods();
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Cave-only defaults reset did not render");
  const restoreDefaults = element<HTMLButtonElement>(".dst-mod-section-head .ghost-button");
  assert(!restoreDefaults.disabled, "Restore all defaults remains available for a caves-only override");
  await click(restoreDefaults);
  await settle(() => !(settings().caves_mod_configuration_options as Record<string, unknown>)[id], "Caves-only override was not reset");
  for (const shard of ["master", "caves"]) {
    const configuration = settings()[`${shard}_mod_configuration_options`] as Record<string, Record<string, unknown>>;
    assert(!configuration[id] && configuration[unrelatedId].other === (shard === "master"),
      `${shard} defaults reset clears only the selected Mod's configuration`);
  }
  setSettings(caveOnlySettings);
  await render(true);
  await myMods();
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Final configuration did not render");
  const panel = element(".mw-detail-config-col").getBoundingClientRect();
  assert(panel.width > 300 && panel.height > 200, "Real Mod workspace styles provide a usable configuration column");
  const field = optionsControl("Difficulty").getBoundingClientRect();
  assert(field.width > 80 && field.top >= 0 && field.bottom <= innerHeight, "Saved disabled Mod options are visible in the viewport");
  assert(document.documentElement.scrollWidth <= innerWidth + 1, "Workspace does not overflow the desktop viewport");
  offline = false;
  rateLimited = true;
  await render(true);
  await myMods();
  await settle(() => fixture.textContent?.includes("RATE_LIMIT_END") === true && Boolean(fixture.querySelector(".dst-mod-spec-list")),
    "Final rate-limit screenshot state did not preserve the local configuration");
  const syncNotice = [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice-text")]
    .find((notice) => notice.textContent?.startsWith("Existing shard values differ"));
  assert(syncNotice && syncNotice.title.includes("the next change will synchronize this Mod configuration") && syncNotice.title === syncNotice.textContent,
    "Truncated shard warning retains the complete synchronization message in its title and accessible text");
  assert(errors.length === 0, "Browser console and uncaught errors remain empty");
  return { status: "passed", checks, writes, downloads, browser_errors: errors,
    synchronization_notice: { text: syncNotice.textContent, title: syncNotice.title,
      truncated: syncNotice.scrollWidth > syncNotice.clientWidth },
    viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Mod browser acceptance exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    return fetch(nonce ? `/__reliability_result/${nonce}` : "/__mod_result", { method: "POST", body: JSON.stringify(report) });
  });
