import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ConfigurationField } from "../../src/views/settings/ConfigurationField";
import type { GuidedSettingsField } from "../../src/views/settings/settings-schema";
import type { DstModConfigOptionSpec, InstanceDetails } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column;gap:12px";
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [];
const moduleDetails = buildMockModuleDetails("dontstarve"), modId = "123456";
let details: InstanceDetails, writes = 0;
const options: DstModConfigOptionSpec[] = [
  { name: "difficulty", label: "Difficulty", default_value: { kind: "number", value: 1 }, options: [
    { label: "Normal", value: { kind: "number", value: 1 } },
    { label: "Very challenging survival for experienced players across both shards", value: { kind: "number", value: 10 } }] },
  { name: "message", label: "Welcome message", default_value: { kind: "string", value: "Welcome" }, options: [] },
  { name: "limit", label: "Player limit", default_value: { kind: "number", value: 20 }, options: [] },
  ...Array.from({ length: 9 }, (_, index): DstModConfigOptionSpec => ({ name: `rule_${index}`,
    label: index === 0 ? "A deliberately long setting label for multiplayer resource regeneration behavior" : `Resource rule ${index + 1}`,
    default_value: { kind: "boolean", value: true }, options: [] }))
];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(value: unknown, label: string): asserts value { if (!value) throw new Error(label); checks.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const result = within.querySelector<T>(selector); if (!result) throw new Error(`Missing ${selector}`); return result;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function nativeKey(name: "Tab" | "Enter") {
  await act(async () => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    assert(response.ok, `Native ${name} was dispatched`);
  });
}
const presentation = { state: "editable", owner: "configuration", sectionId: "room" } as const;
const fields: GuidedSettingsField[] = [
  { key: "server_name", title: "Global text field", type: "string", control: "text", required: false, sectionId: "room", presentation },
  { key: "mode", title: "Global select field", type: "string", control: "select", required: false, sectionId: "room", presentation,
    enumOptions: [{ label: "Normal", value: "normal" }, { label: "Survival", value: "survival" }] }
];
function Reference() {
  const [values, setValues] = useState<Record<string, unknown>>({ server_name: "Reference", mode: "normal" });
  return <section className="configuration-workspace" id="global-reference" style={{ display: "block", height: "auto", flex: "0 0 auto" }}>
    <div className="settings-schema-section"><div className="settings-schema-grid" style={{ gridTemplateColumns: "1fr 1fr auto", gap: 12 }}>
      {fields.map((field) => <ConfigurationField key={field.key} field={field} value={values[field.key]} settings={values}
        onPatch={(patch) => setValues((current) => ({ ...current, ...patch }))}
        copy={{ concealSecret: "Hide", revealSecret: "Show", restartScopes: { none: "", server: "", world: "", cluster: "" } }} />)}
      <div className="settings-editor-toolbar" style={{ alignSelf: "end" }}><button type="button" className="ghost-button">Global button</button></div>
    </div></div>
  </section>;
}
function draw() { root.render(<><Reference /><div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
  <ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={0} onSaved={draw} />
</div></>); }
function metrics(node: HTMLElement) {
  const style = getComputedStyle(node), bounds = node.getBoundingClientRect();
  return { fontSize: style.fontSize, lineHeight: style.lineHeight, fontWeight: style.fontWeight, height: bounds.height,
    minHeight: style.minHeight, padding: style.padding, paddingLeft: style.paddingLeft, paddingRight: style.paddingRight,
    borderRadius: style.borderRadius, outlineStyle: style.outlineStyle, boxShadow: style.boxShadow };
}
async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const savedOptions = { difficulty: 10, message: "Welcome back", limit: 24 };
  details = { ...template, summary: { ...template.summary, id: "fixture-density", status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify({ enable_caves: true, shared_workshop_mod_ids: modId,
      master_enabled_workshop_mod_ids: modId, caves_enabled_workshop_mod_ids: modId,
      master_mod_configuration_options: { [modId]: savedOptions }, caves_mod_configuration_options: { [modId]: savedOptions } }) };
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog: [workshopItem(modId, "Server configuration controls")], cached: new Set([modId]) }), {
    check: assert, save: (saved) => { details = saved; writes += 1; }
  });
  mockWindows("mod-config-density");
  mockIPC((command, payload) => command === "read_dontstarve_mod_configuration_specs"
    ? [{ mod_id: modId, client_only: false, status: "loaded", options }] : baseIpc(command, payload), { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
  const myMods = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "My Mods")!;
  await act(async () => { myMods.click(); });
  await settle(() => fixture.querySelectorAll(".dst-mod-spec-row").length === options.length, "Real Mod option fields did not load");
  const modSelect = element<HTMLSelectElement>(".dst-mod-spec-row select"), modInput = element<HTMLInputElement>('.dst-mod-spec-row input[type="text"]');
  const globalInput = element<HTMLInputElement>("#global-reference input"), globalSelect = element<HTMLSelectElement>("#global-reference select");
  const panel = element(".mw-detail-config-col");
  const controls = { modLabel: element(".dst-mod-spec-label"), modInput, modSelect,
    modNumber: element<HTMLInputElement>('.dst-mod-spec-row input[type="number"]'),
    shardSelect: element(".dst-mod-shard-field select"), reset: element(".dst-mod-section-head > .ghost-button"),
    tab: myMods, globalLabel: element("#global-reference .settings-field-label"), globalInput, globalSelect,
    globalButton: element("#global-reference .ghost-button") };
  const computed = Object.fromEntries(Object.entries(controls).map(([name, node]) => [name, metrics(node)]));
  assert(modSelect.selectedOptions[0].textContent?.startsWith("Very challenging") && modSelect.value === "number:10", "Long Mod option labels preserve their selected value");
  const row = modSelect.closest<HTMLElement>(".dst-mod-spec-row")!;
  assert(modSelect.getBoundingClientRect().right <= row.getBoundingClientRect().right + 1, "Long select fits its configuration row");
  await act(async () => { modSelect.scrollIntoView({ block: "nearest" }); modSelect.focus(); });
  await nativeKey("Tab");
  assert(document.activeElement === modInput, "Native Tab moves from the Mod select to its next text input");
  const focused = metrics(modInput);
  assert(focused.boxShadow !== "none" || focused.outlineStyle !== "none", "Focused Mod control retains a visible focus indicator");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(modInput, "Configured in browser");
    modInput.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await nativeKey("Enter");
  await settle(() => writes > 0, "Mod text change did not save");
  assert(JSON.parse(details.settings_json).master_mod_configuration_options[modId].message === "Configured in browser", "Density fixture saves through the real Mod settings path");
  const last = [...fixture.querySelectorAll<HTMLSelectElement>(".dst-mod-spec-row select")].at(-1)!;
  await act(async () => { last.scrollIntoView({ block: "nearest" }); last.focus(); });
  const lastBounds = last.getBoundingClientRect(), panelBounds = panel.getBoundingClientRect();
  const lastOptionVisible = lastBounds.top >= Math.max(0, panelBounds.top) - 1 && lastBounds.bottom <= Math.min(innerHeight, panelBounds.bottom) + 1;
  const viewportFits = document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1;
  await act(async () => {
    // scrollIntoView may also scroll an overflow:hidden ancestor; restore the
    // whole fixture viewport after the end-of-list keyboard/scroll acceptance.
    for (let owner = modSelect.parentElement; owner && owner !== fixture; owner = owner.parentElement) owner.scrollTop = 0;
    modSelect.focus({ preventScroll: true });
  });
  assert(errors.length === 0, "Density browser has no console or uncaught errors");
  return { status: "passed", checks, computed, focused, writes, last_option_visible: lastOptionVisible, viewport_fits: viewportFits,
    scroll_geometry: { last: lastBounds.toJSON(), panel: panelBounds.toJSON(), scrollHeight: document.documentElement.scrollHeight },
    browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Density acceptance exceeded 30 seconds")), 30000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
