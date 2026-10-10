import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/valheim/schema.json";
import { I18nProvider } from "../../src/i18n";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsDraftInvalidError } from "../../src/views/settings/instance-settings-save-queue";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import { ValheimWorldRulesPanel } from "../../src/views/settings/ValheimWorldRulesPanel";
import { ValheimWorldRulesProvider } from "../../src/views/settings/ValheimWorldRulesContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const calls: UpdateInstanceInput[] = [];
const overflowViolations: string[] = [];
const locales = ["en-US", "zh-CN"] as const;
const modifierChoices = {
  combat: ["", "veryeasy", "easy", "hard", "veryhard"],
  deathpenalty: ["", "casual", "veryeasy", "easy", "hard", "hardcore"],
  resources: ["", "muchless", "less", "more", "muchmore", "most"],
  raids: ["", "none", "muchless", "less", "more", "muchmore"],
  portals: ["", "casual", "hard", "veryhard"]
};
const modifierKeys = Object.keys(modifierChoices) as Array<keyof typeof modifierChoices>;
const worldKeys = ["nobuildcost", "playerevents", "passivemobs", "nomap"];
const controlKeys = ["world_preset", ...modifierKeys.map((key) => `valheim_modifier_${key}`),
  ...worldKeys.map((key) => `valheim_key_${key}`)];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };
let nativeKeys: string[] = [];
let nativeSource: "saved" | "missing_metadata" = "saved";
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
  if (command !== "read_valheim_world_rules") throw new Error(`Unexpected native command: ${command}`);
  return { instance_id: args.instanceId, world_name: args.worldName, source: nativeSource,
    world_version: nativeSource === "saved" ? 41 : null, saved_keys: structuredClone(nativeKeys) };
} } });

const moduleDetails: ModuleDetails = {
  summary: { id: "valheim", name: "Valheim", version: "1.0", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify(schema), default_ports: [], runtime: {}
};
const instance: InstanceDetails = {
  summary: { id: "valheim-world-rules", module_id: "valheim", name: "Retained Valheim world", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
  settings_json: "{}", ports: [], config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3, active_run: null
};
const initialSettings = { server_name: "Retained Valheim world", world_name: "RetainedWorld",
  server_password: "fixture-password", future_setting: "retained" };
let saved = JSON.stringify(initialSettings);
let flush: () => Promise<void>;
let disabledPatches = 0;
let malformedReadback: Record<string, unknown> = {};
const malformedPatches: Readonly<Record<string, unknown>>[] = [];
function Workspace() {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [details, setDetails] = useState({ ...instance, settings_json: saved });
  flush = () => coordinator.flush(instance.summary.id);
  return <ConfigurationWorkspace details={details} moduleDetails={moduleDetails} moduleDetailsError={null}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null} onSave={async (input, options) => {
      if (input.id !== instance.summary.id || !sameSettings(options?.expectedSettingsJson, saved)) {
        throw new Error("Unexpected configuration save baseline");
      }
      calls.push(structuredClone(input));
      saved = input.settings_json;
      const readback = { ...instance, settings_json: saved };
      setDetails(readback);
      return readback;
    }} />;
}
function MalformedSettings({ initial }: { initial: Record<string, unknown> }) {
  const [values, setValues] = useState(initial);
  return <ValheimWorldRulesProvider details={{ ...instance, settings_json: JSON.stringify(initial) }}>
    <ValheimWorldRulesPanel sectionId="world" details={{ ...instance, settings_json: JSON.stringify(initial) }}
    moduleDetails={moduleDetails} settings={values} disabled={false} onPatch={(patch) => {
      malformedPatches.push(structuredClone(patch));
      setValues((current) => { malformedReadback = { ...current, ...patch }; return malformedReadback; });
    }} /></ValheimWorldRulesProvider>;
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends Element>(selector: string): T {
  const result = fixture?.querySelector<T>(selector);
  if (!result) throw new Error(`Missing control: ${selector}; ${fixture?.textContent}`);
  return result;
}
function control(key: string) {
  return element<HTMLSelectElement | HTMLInputElement>(`[data-field-key="${key}"] select,[data-field-key="${key}"] input,[data-valheim-rule="${key}"] select,[data-valheim-rule="${key}"] input`);
}
function preset() { return element<HTMLSelectElement>('[data-field-key="world_preset"] select'); }
function modifier(key: string) { return element<HTMLSelectElement>(`[data-valheim-rule="valheim_modifier_${key}"] select`); }
function toggle(key: string) { return element<HTMLInputElement>(`[data-valheim-rule="valheim_key_${key}"] input[type="checkbox"]`); }
function settings(): Record<string, unknown> { return JSON.parse(saved); }
function entries(value: unknown) { return typeof value === "string" ? value.split(/[\r\n,;]+/).map((entry) => entry.trim()).filter(Boolean) : []; }
function sameEntries(actual: unknown, expected: string[]) {
  return JSON.stringify(entries(actual).sort()) === JSON.stringify([...expected].sort());
}
function sameSettings(left: string | undefined, right: string) {
  if (left === undefined) return false;
  const first = JSON.parse(left);
  const second = JSON.parse(right);
  return JSON.stringify(Object.keys(first).sort()) === JSON.stringify(Object.keys(second).sort()) &&
    Object.keys(first).every((key) => JSON.stringify(first[key]) === JSON.stringify(second[key]));
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function draw(key: string) {
  await act(async () => { root.render(<I18nProvider key={key}><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}><Workspace /></main>
  </InstanceSettingsSaveProvider></I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture?.querySelector(".configuration-workspace__body")) {
    if (performance.now() >= deadline) throw new Error(`Workspace did not load: ${fixture?.textContent}`);
    await act(frame);
  }
}
async function section(id: string) {
  await act(async () => { element<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`).click(); });
  await act(async () => { await document.fonts.ready; await frame(); });
}
async function searchRule(locale: string, query: string, aggregateKey: string, nativeKey: string) {
  await section("room");
  const navigationToggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (navigationToggle.getClientRects().length > 0 && navigationToggle.getAttribute("aria-expanded") !== "true") {
    await act(async () => { navigationToggle.click(); });
  }
  const search = element<HTMLInputElement>('.configuration-search input[type="search"]');
  check(search.getClientRects().length > 0, `${locale}: configuration search is visible`);
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!setter) throw new Error("Search input setter is unavailable");
  await act(async () => { setter.call(search, query); search.dispatchEvent(new Event("input", { bubbles: true })); });
  const hit = [...fixture!.querySelectorAll<HTMLButtonElement>(".configuration-search-results li > button")]
    .find((button) => button.querySelector("code")?.textContent === nativeKey);
  check(hit, `${locale}: searching ${query} discovers its native ${aggregateKey} group`);
  await act(async () => { hit.click(); await frame(); });
  const selected = element<HTMLButtonElement>('[data-configuration-section-id="world"] > button');
  const anchor = element<HTMLElement>(`#configuration-valheim-${aggregateKey.replaceAll("_", "-")}-input`);
  check(selected.getAttribute("aria-current") === "page" && Boolean(anchor.querySelector("select,input[type='checkbox']")),
    `${locale}: clicking ${query} opens the world section and registered rule group`);
  check(document.activeElement === anchor || (document.activeElement !== null && anchor.contains(document.activeElement)),
    `${locale}: searching ${query} focuses its registered rule group`);
  check(search.value === "" && !fixture?.querySelector(".configuration-search-results"),
    `${locale}: selecting a search result clears the search without changing rules`);
}
async function passwordHelp(locale: string) {
  await section("room");
  const input = element<HTMLInputElement>('[data-field-key="server_password"] input');
  check(input.type === "password" && input.minLength === 5, `${locale}: password control keeps its native minimum`);
  await act(async () => { input.focus(); });
  const deadline = performance.now() + 4000;
  let tooltip: HTMLElement | null = null;
  while (!(tooltip = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")) ||
    getComputedStyle(tooltip).opacity !== "1") {
    check(performance.now() < deadline, `${locale}: password tooltip becomes visible`);
    await act(frame);
  }
  check(tooltip.textContent?.includes(locale === "zh-CN" ? "至少 5 个字符" : "at least 5 characters"),
    `${locale}: password tooltip explains the minimum before editing`);
  const bounds = tooltip.getBoundingClientRect();
  check(bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight,
    `${locale}: password help fits the desktop viewport`);
  return input;
}
async function editPassword(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!setter) throw new Error("Password input setter is unavailable");
  await act(async () => { setter.call(input, value); input.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function verifyPassword(locale: string) {
  const input = await passwordHelp(locale);
  const original = settings().server_password;
  const before = calls.length;
  for (const value of ["", "1234", "😀😀😀"]) {
    await editPassword(input, value);
    check(input.getAttribute("aria-invalid") === "true", `${locale}: a short password is marked invalid`);
    let rejected = false;
    try { await save(); } catch (error) {
      if (!(error instanceof InstanceSettingsDraftInvalidError)) throw error;
      rejected = true;
    }
    check(rejected, `${locale}: starting with an invalid password draft is refused`);
    check(calls.length === before && settings().server_password === original,
      `${locale}: invalid password drafts never reach autosave`);
  }
  await editPassword(input, "12345");
  await save();
  check(!input.hasAttribute("aria-invalid") && calls.length === before + 1 && settings().server_password === "12345",
    `${locale}: a five-character password saves successfully`);
  await editPassword(input, String(original));
  await save();
}
async function change(select: HTMLSelectElement, value: string) {
  check(!select.disabled && select.getClientRects().length > 0, `Editable select ${select.id}`);
  check([...select.options].some((option) => option.value === value), `Select offers native value ${value || "inherit"}`);
  await act(async () => { select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function tick(key: string, value: boolean) {
  const input = toggle(key);
  check(!input.disabled && Boolean(input.labels?.length), `Editable labeled world key ${key}`);
  await act(async () => { if (input.checked !== value) input.labels![0].click(); });
  check(toggle(key).checked === value, `World key ${key} changes through its visible label`);
}
async function save() { await act(async () => { await flush(); }); }
function absent() { return ["world_preset", "world_modifiers", "world_set_keys"].every((key) => !Object.hasOwn(settings(), key)); }
function preservesSettings(locale: string) {
  const values = settings();
  check(values.server_name === `Retained ${locale} world` && values.world_name === initialSettings.world_name &&
    values.server_password === initialSettings.server_password && values.future_setting === initialSettings.future_setting,
  `${locale}: world rule edits preserve unrelated configuration`);
  check(!Object.keys(values).some((key) => key.startsWith("valheim_modifier_") || key.startsWith("valheim_key_")),
    `${locale}: synthetic UI fields never enter persisted settings`);
}
function layout(locale: string) {
  const content = element<HTMLElement>(".configuration-workspace__main");
  const bounds = content.getBoundingClientRect();
  if (content.scrollWidth > content.clientWidth + 1) overflowViolations.push(`${locale}: configuration scrolls horizontally`);
  for (const key of controlKeys) {
    const container = element<HTMLElement>(`[data-field-key="${key}"],[data-valheim-rule="${key}"]`);
    const box = container.getBoundingClientRect();
    if (box.width <= 0 || box.left < bounds.left - 1 || box.right > bounds.right + 1 || container.scrollWidth > container.clientWidth + 1) {
      overflowViolations.push(`${locale}: ${key} escapes its content width`);
    }
  }
  check(overflowViolations.length === 0, `${locale}: world rules remain within the ${innerWidth}px workspace`);
}
function labels(locale: string) {
  for (const key of controlKeys) {
    const input = control(key);
    const label = input.labels?.[0]?.textContent?.trim();
    check(Boolean(label), `${locale}: ${key} has an associated visible label`);
    check(locale === "zh-CN" ? /[\u4e00-\u9fff]/.test(label!) : !/[\u4e00-\u9fff]/.test(label!),
      `${locale}: ${key} uses the active language`);
  }
}
async function verifyDisabled(locale: string) {
  const before = disabledPatches;
  const values = settings();
  await act(async () => { root.render(<I18nProvider key={`${locale}-disabled`}>
    <main className="server-detail-panel configuration-workspace" style={{ height: "calc(100vh - 32px)", margin: 16, padding: 24 }}>
      <ValheimWorldRulesProvider details={{ ...instance, settings_json: saved }}>
        <ValheimWorldRulesPanel sectionId="world" details={{ ...instance, settings_json: saved }} moduleDetails={moduleDetails}
          settings={values} disabled onPatch={() => { disabledPatches++; }} />
      </ValheimWorldRulesProvider>
    </main>
  </I18nProvider>); await frame(); });
  const states = controlKeys.map((key) => { const input = control(key); return input instanceof HTMLSelectElement ? input.value : input.checked; });
  for (const key of controlKeys) {
    const input = control(key);
    check(input.disabled, `${locale}: disabled rule ${key} exposes native disabled state`);
    await act(async () => { input.click(); input.focus(); });
    check(document.activeElement !== input, `${locale}: disabled rule ${key} cannot receive focus`);
  }
  check(disabledPatches === before && JSON.stringify(states) === JSON.stringify(controlKeys.map((key) => {
    const input = control(key); return input instanceof HTMLSelectElement ? input.value : input.checked;
  })), `${locale}: disabled controls do not change values or request patches`);
}
async function verifyMalformed(locale: string, fieldKey: "world_modifiers" | "world_set_keys") {
  const invalid = fieldKey === "world_modifiers" ? ["combat hard"] : ["nomap"];
  const initial = { ...initialSettings, world_modifiers: "resources most", world_set_keys: "playerevents", [fieldKey]: invalid };
  malformedReadback = initial;
  const before = malformedPatches.length;
  await act(async () => { root.render(<I18nProvider key={`${locale}-${fieldKey}-malformed`}>
    <main className="server-detail-panel configuration-workspace" style={{ height: "calc(100vh - 32px)", margin: 16, padding: 24 }}>
      <MalformedSettings initial={initial} />
    </main>
  </I18nProvider>); await frame(); });
  const group = element<HTMLElement>(`[data-field-key="${fieldKey}"]`);
  const inputs = [...group.querySelectorAll<HTMLInputElement | HTMLSelectElement>("select,input[type='checkbox']")];
  const otherKey = fieldKey === "world_modifiers" ? "world_set_keys" : "world_modifiers";
  check(inputs.length === (fieldKey === "world_modifiers" ? 5 : 4) && inputs.every((input) => input.disabled),
    `${locale}: malformed ${fieldKey} disables only its affected rule controls`);
  check(!preset().disabled && [...element<HTMLElement>(`[data-field-key="${otherKey}"]`)
    .querySelectorAll<HTMLInputElement | HTMLSelectElement>("select,input[type='checkbox']")].every((input) => !input.disabled),
  `${locale}: malformed ${fieldKey} keeps other rule groups editable`);
  await act(async () => { for (const input of inputs) { input.click(); input.labels?.[0]?.click(); } });
  check(malformedPatches.length === before && JSON.stringify(malformedReadback[fieldKey]) === JSON.stringify(invalid),
    `${locale}: clicking disabled ${fieldKey} controls preserves the original array`);
  const repair = element<HTMLButtonElement>(`[data-field-key="${fieldKey}"] [role="alert"] button`);
  check(!repair.disabled && Boolean(repair.textContent?.trim()), `${locale}: malformed ${fieldKey} exposes an explicit repair action`);
  await act(async () => { repair.click(); });
  check(malformedPatches.length === before + 1 && JSON.stringify(malformedPatches.at(-1)) === JSON.stringify({ [fieldKey]: "" }) &&
    malformedReadback[otherKey] === initial[otherKey] && malformedReadback.future_setting === initial.future_setting,
    `${locale}: explicit repair clears only malformed ${fieldKey} and retains other settings`);
  check([...element<HTMLElement>(`[data-field-key="${fieldKey}"]`).querySelectorAll<HTMLInputElement | HTMLSelectElement>("select,input[type='checkbox']")]
    .every((input) => !input.disabled), `${locale}: repairing ${fieldKey} restores its native choices`);
}

Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const locale of locales) {
    localStorage.setItem("langame.locale", locale);
    saved = JSON.stringify(initialSettings);
    const before = calls.length;
    await draw(`${locale}-initial`);
    await section("world");
    check(preset().value === "" && modifierKeys.every((key) => modifier(key).value === "") && worldKeys.every((key) => !toggle(key).checked),
      `${locale}: absent startup overrides retain the saved world's rules`);
    check(preset().selectedOptions[0].textContent?.includes(locale === "zh-CN" ? "标准" : "Normal") &&
      modifierKeys.every((key) => modifier(key).selectedOptions[0].textContent === (locale === "zh-CN" ? "标准" : "Normal")),
    `${locale}: metadata-backed normal rules show concrete current choices`);
    check(!fixture?.textContent?.includes(locale === "zh-CN" ? "沿用预设或存档" : "Use preset or saved rules"),
      `${locale}: selected rules never use the ambiguous inheritance label`);
    check(JSON.stringify([...preset().options].map((option) => option.value)) ===
      JSON.stringify(["", "normal", "casual", "easy", "hard", "hardcore", "immersive", "hammer"]),
    `${locale}: all native presets remain selectable`);
    for (const key of modifierKeys) {
      check(JSON.stringify([...modifier(key).options].map((option) => option.value)) === JSON.stringify(modifierChoices[key]),
        `${locale}: ${key} offers only native modifier values and inheritance`);
    }
    check(!fixture?.querySelector('[data-field-key="world_modifiers"] input:not([type="checkbox"]),[data-field-key="world_modifiers"] textarea,' +
      '[data-field-key="world_set_keys"] input:not([type="checkbox"]),[data-field-key="world_set_keys"] textarea'),
    `${locale}: world modifiers and keys require no manual entry`);
    labels(locale);
    layout(locale);
    await save();
    check(calls.length === before && absent(), `${locale}: opening and navigation never persist default rules`);
    for (const [query, aggregateKey, nativeKey] of locale === "zh-CN"
      ? [["战斗难度", "world_modifiers", "-modifier"], ["免费建造", "world_set_keys", "-setkey"]]
      : [["Combat Difficulty", "world_modifiers", "-modifier"], ["Free Building", "world_set_keys", "-setkey"]]) {
      await searchRule(locale, query, aggregateKey, nativeKey);
    }
    await save();
    check(calls.length === before && absent(), `${locale}: searching individual rule names does not create overrides`);
    await section("room");
    const name = element<HTMLInputElement>('[data-field-key="server_name"] input');
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (!setter) throw new Error("Input setter is unavailable");
    await act(async () => { setter.call(name, `Retained ${locale} world`); name.dispatchEvent(new Event("input", { bubbles: true })); });
    await save();
    check(calls.length === before + 1 && absent(), `${locale}: saving another field leaves all world rules unmanaged`);
    await section("world");
    await change(modifier("combat"), "hard");
    await change(modifier("resources"), "most");
    await tick("nomap", true);
    await save();
    check(typeof settings().world_modifiers === "string" && sameEntries(settings().world_modifiers, ["combat hard", "resources most"]) &&
      settings().world_set_keys === "nomap", `${locale}: combat, resources and map choice save as native rule strings`);
    check(!settings().world_preset, `${locale}: individual choices do not silently select a preset`);
    preservesSettings(locale);
    await draw(`${locale}-saved-readback`);
    await section("world");
    check(modifier("combat").value === "hard" && modifier("resources").value === "most" && toggle("nomap").checked && preset().value === "",
      `${locale}: saved rule choices survive configuration remount`);
    await change(modifier("deathpenalty"), "casual");
    await change(modifier("raids"), "less");
    await change(modifier("portals"), "hard");
    for (const key of ["nobuildcost", "playerevents", "passivemobs"]) await tick(key, true);
    await save();
    const fullModifiers = ["combat hard", "deathpenalty casual", "resources most", "raids less", "portals hard"];
    check(sameEntries(settings().world_modifiers, fullModifiers) && sameEntries(settings().world_set_keys, worldKeys),
      `${locale}: every modifier and toggle composes without overwriting another rule`);
    const originalModifiers = settings().world_modifiers;
    const originalKeys = settings().world_set_keys;
    await change(preset(), "hard");
    await save();
    check(settings().world_preset === "hard" && settings().world_modifiers === originalModifiers && settings().world_set_keys === originalKeys,
      `${locale}: changing the preset preserves explicit per-rule overrides`);
    await draw(`${locale}-preset-readback`);
    await section("world");
    check(preset().value === "hard" && modifierKeys.every((key) => modifier(key).value === (key === "deathpenalty" ? "casual" : key === "resources" ? "most" : key === "raids" ? "less" : "hard")) &&
      worldKeys.every((key) => toggle(key).checked), `${locale}: preset and all rule overrides survive reopening`);
    await change(modifier("combat"), "");
    await tick("nomap", false);
    await save();
    check(settings().world_preset === "hard" && sameEntries(settings().world_modifiers, fullModifiers.filter((entry) => entry !== "combat hard")) &&
      sameEntries(settings().world_set_keys, worldKeys.filter((key) => key !== "nomap")),
    `${locale}: releasing one modifier and key deletes only those overrides`);
    await change(preset(), "");
    await save();
    check(!settings().world_preset && sameEntries(settings().world_modifiers, fullModifiers.filter((entry) => entry !== "combat hard")),
      `${locale}: releasing the preset never writes normal or resets other rules`);
    await draw(`${locale}-released-readback`);
    await section("world");
    check(preset().value === "" && modifier("combat").value === "" && !toggle("nomap").checked && modifier("resources").value === "most" && toggle("playerevents").checked,
      `${locale}: released rules remain inherited on reopening while other choices persist`);
    preservesSettings(locale);
    labels(locale);
    layout(locale);
    await verifyDisabled(locale);
    await verifyMalformed(locale, "world_modifiers");
    await verifyMalformed(locale, "world_set_keys");

    nativeKeys = ["playerdamage 85", "enemydamage 150", "enemyspeedsize 110", "enemyleveluprate 120", "nomap", "resourcerate 300"];
    saved = JSON.stringify(initialSettings);
    await draw(`${locale}-native-custom`);
    await section("world");
    check(modifier("combat").selectedOptions[0].textContent === (locale === "zh-CN" ? "困难" : "Hard") &&
      modifier("resources").selectedOptions[0].textContent === (locale === "zh-CN" ? "3 倍" : "3 ×"),
    `${locale}: the saved custom world's concrete combat and resource choices are displayed`);
    check(toggle("nomap").checked && toggle("nomap").disabled && !toggle("nobuildcost").checked && !toggle("nobuildcost").disabled,
      `${locale}: an enabled saved rule stays visibly enabled without an unsupported disable action`);
    const beforeRead = calls.length;
    await save();
    check(calls.length === beforeRead && absent(), `${locale}: reading concrete native rules never creates launch overrides`);
    await change(preset(), "normal");
    await save();
    check(!toggle("nomap").checked && !toggle("nomap").disabled &&
      fixture?.textContent?.includes(locale === "zh-CN" ? "当前存档：开启 · 重启后：关闭" : "Saved world: On · After restart: Off"),
    `${locale}: selecting Normal shows the saved-to-startup rule change truthfully`);
    nativeKeys = ["preset hammer"];
    saved = JSON.stringify(initialSettings);
    await draw(`${locale}-native-preset-tag`);
    await section("world");
    check(toggle("nobuildcost").checked && toggle("nobuildcost").disabled && toggle("passivemobs").checked &&
      modifier("raids").selectedOptions[0].textContent === (locale === "zh-CN" ? "无袭击" : "No Raids"),
    `${locale}: serialized preset tags expand through the native key contract before rendering effective rules`);
    nativeKeys = [];
    nativeSource = "missing_metadata";
    saved = JSON.stringify(initialSettings);
    await draw(`${locale}-native-missing`);
    await section("world");
    check(modifier("combat").selectedOptions[0].textContent === (locale === "zh-CN" ? "尚未读取" : "Not read") &&
      toggle("nomap").indeterminate && toggle("nomap").disabled,
    `${locale}: missing metadata is an unknown rule value rather than a fabricated Normal or Off state`);
    nativeSource = "saved";
    await verifyPassword(locale);
  }
  await draw("zh-CN-final-preview");
  await section("world");
  check(errors.length === 0, "The browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", locales, checks, browser_errors: errors, overflow_violations: overflowViolations };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors,
  overflow_violations: overflowViolations }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
