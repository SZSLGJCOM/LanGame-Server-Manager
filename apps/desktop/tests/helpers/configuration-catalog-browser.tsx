import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { buildConfigurationWorkspaceModel } from "../../src/views/settings/configuration-workspace-model";
import { parseGuidedSettingsSchema } from "../../src/views/settings/guided-settings";
import { listConfigurationSpecializedRenderers, resolveSettingsModuleDefinition } from "../../src/views/settings/module-registry";
import type { GuidedSettingsSchema } from "../../src/views/settings/settings-schema";
import { CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS } from "./configuration-module-acceptance";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

const persisted = new Map<string, InstanceDetails>();
const saves: UpdateInstanceInput[] = [];
const modules: Array<{ id: string; sections: number; configuration_fields: number; specialized_editors: number }> = [];
let activeSchema: GuidedSettingsSchema | null = null;
let activeModule = "";
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}

function Workspace({ descriptor, initial }: { descriptor: ModuleDetails; initial: InstanceDetails }) {
  const { locale, t } = useI18n();
  const [details, setDetails] = useState(initial);
  activeSchema = parseGuidedSettingsSchema(descriptor, locale, t);
  activeModule = descriptor.summary.id;
  return <ConfigurationWorkspace details={details} moduleDetails={descriptor} moduleDetailsError={null}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null}
    onSave={async (input) => {
      saves.push(structuredClone(input));
      const saved = { ...details, settings_json: input.settings_json, ports: input.ports ?? details.ports,
        summary: { ...details.summary, bind_ip: input.bind_ip } };
      persisted.set(descriptor.summary.id, saved);
      setDetails(saved);
      return saved;
    }} />;
}

async function mount(moduleId: string, settingsOverrides: Record<string, unknown> = {}) {
  let descriptor!: ModuleDetails;
  // The previous workspace can finish a pending save while the next descriptor loads.
  await act(async () => { descriptor = await readModuleDetails(moduleId); });
  const id = `configuration-catalog-${moduleId}`;
  const initial: InstanceDetails = persisted.get(moduleId) ?? {
    summary: { id, module_id: moduleId, name: "Configuration review", status: "Stopped", active_process_count: 0,
      autostart: false, bind_ip: "0.0.0.0" },
    settings_json: JSON.stringify({ ...buildMockSettingsForModule(moduleId, "Configuration review", id), ...settingsOverrides }),
    ports: descriptor.default_ports, config_file_path: "C:/synthetic/configuration/settings.json",
    saves_path: "C:/synthetic/configuration/saves", backup_uses_declared_saves_path: true,
    auto_backup_on_stop: false, backup_retention_count: 5, active_run: null
  };
  activeSchema = null;
  await act(async () => {
    root.render(<I18nProvider><InstanceSettingsSaveProvider key={moduleId}>
      <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}>
        <Workspace key={`${moduleId}-${saves.length}`} descriptor={descriptor} initial={initial} />
      </main>
    </InstanceSettingsSaveProvider></I18nProvider>);
    await frame();
  });
  const deadline = performance.now() + 5000;
  while (activeModule !== moduleId || !activeSchema || !fixture.querySelector(".configuration-workspace")) {
    check(performance.now() < deadline, `${moduleId}: workspace did not mount: ${fixture.textContent}`);
    await act(async () => { await frame(); });
  }
  await act(async () => { await document.fonts.ready; await frame(); });
  check(!fixture.querySelector("[data-configuration-state]"), `${moduleId}: workspace has a local loading/error state`);
  const schema = activeSchema as GuidedSettingsSchema;
  check(!schema.parseError, `${moduleId}: ${schema.parseError}`);
  return { descriptor, schema, model: buildConfigurationWorkspaceModel(schema) };
}

async function selectSection(sectionId: string) {
  const toggle = fixture.querySelector<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle?.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") {
    await act(async () => { toggle.click(); });
  }
  for (let depth = 0; depth < 10; depth++) {
    const collapsed = [...fixture.querySelectorAll<HTMLButtonElement>(
      '.configuration-section-navigation button[aria-expanded="false"]')];
    if (collapsed.length === 0) break;
    await act(async () => { collapsed.forEach((button) => button.click()); });
  }
  const target = fixture.querySelector<HTMLButtonElement>(
    `[data-configuration-section-id="${CSS.escape(sectionId)}"] > .configuration-section-navigation__button`);
  check(target, `${activeModule}.${sectionId}: missing actionable navigation`);
  await act(async () => { target.click(); await frame(); });
  check(target.getAttribute("aria-current") === "page", `${activeModule}.${sectionId}: category did not activate`);
}

function verifySection(moduleId: string, sectionId: string, schema: GuidedSettingsSchema) {
  const expected = (schema.presentationFields ?? schema.fields).filter((field) =>
    field.sectionId === sectionId && field.presentation.owner === "configuration" &&
    ["editable", "specialized"].includes(field.presentation.state)).map((field) => field.key).sort();
  const renderers = listConfigurationSpecializedRenderers(resolveSettingsModuleDefinition(moduleId), sectionId);
  const cards = [...fixture.querySelectorAll<HTMLElement>(".configuration-workspace__main [data-field-key]")];
  const actual = cards.map((card) => card.dataset.fieldKey!).sort();
  check(new Set(actual).size === actual.length && actual.every((key) => expected.includes(key)),
    `${moduleId}.${sectionId}: controls contain duplicate or unexpected fields: ${actual}`);
  for (const key of expected) {
    if (actual.includes(key)) continue;
    const registration = renderers.find((renderer) => renderer.fieldKey === key);
    const inputId = `configuration-${moduleId}-${key.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}-input`;
    check(registration && fixture.querySelectorAll(`#${CSS.escape(inputId)}.configuration-workspace__addon`).length === 1,
      `${moduleId}.${sectionId}.${key}: missing field control or registered structured editor`);
  }
  const foreign = new Set((schema.presentationFields ?? []).filter((field) => field.presentation.owner !== "configuration")
    .map((field) => field.key));
  check(actual.every((key) => !foreign.has(key)), `${moduleId}.${sectionId}: another workspace owns a rendered control`);
  for (const card of cards) {
    const labelledBy = card.getAttribute("aria-labelledby")?.split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent?.trim()).filter(Boolean).join(" ");
    const label = labelledBy || card.getAttribute("aria-label")?.trim()
      || card.querySelector<HTMLElement>(".settings-field-label,.settings-field-title")?.textContent?.trim();
    check(label && !/^settings\.[\w.]+$/.test(label), `${moduleId}.${card.dataset.fieldKey}: missing translated title`);
    check(card.querySelector("input,select,textarea,button"), `${moduleId}.${card.dataset.fieldKey}: missing operable control`);
  }
  check(fixture.querySelectorAll(".configuration-workspace__addon").length === renderers.length,
    `${moduleId}.${sectionId}: specialized editors must render exactly once`);
  check(document.documentElement.scrollWidth <= innerWidth + 1,
    `${moduleId}.${sectionId}: configuration overflows the viewport horizontally`);
  return { fields: expected.length, specialized: renderers.length };
}

async function verifySaveReadback(moduleId: string, fieldKey: string, value: number | string,
  settingsOverrides: Record<string, unknown> = {}) {
  const { schema } = await mount(moduleId, settingsOverrides);
  const field = schema.fields.find((entry) => entry.key === fieldKey);
  check(field, `${moduleId}.${fieldKey}: field is missing`);
  await selectSection(field.sectionId);
  const selector = `[data-field-key="${field.key}"] input, [data-field-key="${field.key}"] select, [data-field-key="${field.key}"] textarea`;
  const input = fixture.querySelector<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>(selector);
  check(input && !input.disabled, `${moduleId}.${fieldKey}: input must be editable`);
  const controlValue = input instanceof HTMLSelectElement
    ? String(field.enumOptions?.findIndex((option) => option.value === value)) : String(value);
  check(controlValue !== "-1" && controlValue !== "undefined", `${moduleId}.${fieldKey}: option is missing`);
  const before = saves.length;
  await act(async () => {
    const prototype = input instanceof HTMLSelectElement ? HTMLSelectElement.prototype
      : input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(input, controlValue);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  check(input.validity.valid, `${moduleId}.${fieldKey}: native input rejects ${value}`);
  const deadline = performance.now() + 5000;
  while (saves.length === before) {
    check(performance.now() < deadline, `${moduleId}.${fieldKey}: edit did not reach mock save boundary: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 25)); });
  }
  check(saves.length === before + 1, "One edit must produce one debounced save");
  check(JSON.parse(saves.at(-1)!.settings_json)[field.key] === value, `${moduleId}.${fieldKey}: saved value changed`);
  await mount(moduleId);
  await selectSection(field.sectionId);
  check(fixture.querySelector<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>(selector)?.value === controlValue,
    `${moduleId}.${fieldKey}: remount must display the persisted mock response`);
}

async function verifyModuleCatalog(moduleId: string) {
  const { schema, model } = await mount(moduleId);
  check(model.actionableSectionIds[0] === "room", `${moduleId}: room configuration must open first`);
  let guidedFields = 0;
  let specialized = 0;
  for (const sectionId of model.actionableSectionIds) {
    await selectSection(sectionId);
    const result = verifySection(moduleId, sectionId, schema);
    guidedFields += result.fields;
    specialized += result.specialized;
  }
  const configurable = (schema.presentationFields ?? schema.fields).filter((field) =>
    field.presentation.owner === "configuration" && ["editable", "specialized"].includes(field.presentation.state));
  check(guidedFields === configurable.length, `${moduleId}: each configuration setting must appear in one category`);
  modules.push({ id: moduleId, sections: model.actionableSectionIds.length,
    configuration_fields: guidedFields, specialized_editors: specialized });
}

async function run() {
  check(!Object.hasOwn(window, "__TAURI_INTERNALS__"), "Catalogue acceptance must not access a native backend");
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const { mode } = await fetch("/__configuration_review_mode").then((response) => response.json()) as { mode: string };
  check(["catalog", "corekeeper-password", "conan-semantics", "barotrauma-campaign", "barotrauma-network"].includes(mode), "Unknown configuration review mode");
  if (mode === "corekeeper-password") {
    const generated = buildMockSettingsForModule("corekeeper", "Configuration review", "corekeeper-generated-default");
    check(typeof generated.join_password === "string" && generated.join_password.length === 28,
      "Generated Core Keeper password must use the configured default length");
    // Keep the synthetic seed and hash mutually exclusive while exercising password saves.
    const existingPassword = "a".repeat(32);
    const settingsOverrides = { world_seed: "", join_password: existingPassword };
    const { schema } = await mount("corekeeper", settingsOverrides);
    const field = schema.fields.find((entry) => entry.key === "join_password");
    check(field, "Core Keeper join password must be configurable");
    await selectSection(field.sectionId);
    const input = fixture.querySelector<HTMLInputElement>('[data-field-key="join_password"] input');
    check(input && input.maxLength === -1, "Generated password length must not impose an input maximum");
    check(input.value === existingPassword, "Existing Core Keeper password must load without truncation");
    await verifySaveReadback("corekeeper", "join_password", "b".repeat(32), settingsOverrides);
    fixture.querySelector('[data-field-key="join_password"]')?.scrollIntoView({ block: "center" });
    await act(async () => { await frame(); });
    check(errors.length === 0, "Browser and React must report no errors");
    return { status: "passed", generated_password_length: generated.join_password.length,
      password_length: 32, mock_save_readback: true,
      save_boundary: "synthetic onSave; no server files written", browser_errors: errors };
  }
  if (mode === "conan-semantics") {
    await verifyModuleCatalog("conanexiles");
    const edits: Array<[string, number | string]> = [
      ["action_stamina_cost_multiplier", 0.75], ["player_offline_hunger_multiplier", 1.25],
      ["player_water_multiplier", 0.75], ["shield_durability_multiplier", 2],
      ["disabled_knowledge_ids", '("1","2")'], ["storm_time_weekday_start", 1830],
      ["unconscious_time_seconds", 600], ["client_catch_up_time", 14],
      ["max_nudity", 2], ["drop_equipment_on_death", 0]
    ];
    for (const [field, value] of edits) await verifySaveReadback("conanexiles", field, value);
    const { schema } = await mount("conanexiles");
    for (const key of ["player_stamina_cost_multiplier", "validate_phys_nav_walk_with_raycast", "player_building_damage_multiplier"]) {
      check(!schema.fields.some((field) => field.key === key), `${key}: unsupported control must stay excluded`);
    }
    const action = schema.fields.find((field) => field.key === "action_stamina_cost_multiplier")!;
    await selectSection(action.sectionId);
    fixture.querySelector('[data-field-key="action_stamina_cost_multiplier"]')?.scrollIntoView({ block: "center" });
    await act(async () => { await frame(); });
    check(errors.length === 0, "Browser and React must report no errors");
    return { status: "passed", modules, setting_save_readbacks: edits.length,
      save_boundary: "synthetic onSave; no server files written", browser_errors: errors };
  }
  if (mode !== "catalog") {
    await verifySaveReadback("barotrauma", "respawn_interval", 2.5);
    const { schema } = await mount("barotrauma");
    const section = mode.slice("barotrauma-".length);
    await selectSection(section);
    const coverage = verifySection("barotrauma", section, schema);
    const scrollOwner = fixture.querySelector<HTMLElement>("[data-configuration-scroll-owner]");
    if (scrollOwner) scrollOwner.scrollTop = 0;
    await act(async () => { await frame(); });
    check(errors.length === 0, "Browser and React must report no errors");
    return { status: "passed", section, coverage, mock_respawn_interval: 2.5,
      save_boundary: "synthetic onSave; no server files written", browser_errors: errors };
  }
  for (const moduleId of CANONICAL_GAME_CONFIG_ACCEPTANCE_MODULE_IDS) {
    await verifyModuleCatalog(moduleId);
  }
  await verifySaveReadback("abioticfactor", "enemy_deployable_damage_multiplier", 0);
  await verifySaveReadback("satisfactory", "weather_preset", 6);
  for (const field of ["war_event_interval", "war_event_major_duration", "war_event_minor_duration"]) {
    await verifySaveReadback("vrising", field, 7);
  }
  await verifySaveReadback("palworld", "death_penalty", "All");
  const { schema } = await mount("palworld");
  const building = schema.fields.filter((field) => ["max_building_limit_num", "max_building_limit_num_per_player"].includes(field.key));
  check(building.length === 2 && building[0].title !== building[1].title, "Palworld native building limits require distinct labels");
  for (const field of building) {
    await selectSection(field.sectionId);
    verifySection("palworld", field.sectionId, schema);
    check(fixture.querySelector(`[data-field-key="${field.key}"]`), `Palworld ${field.key} must remain reachable`);
  }
  await mount("satisfactory");
  await selectSection("world");
  fixture.querySelector('[data-field-key="weather_preset"]')?.scrollIntoView({ block: "center" });
  await act(async () => { await frame(); });
  check(errors.length === 0, "Browser and React must report no errors");
  return { status: "passed", modules, mock_save_readback: true, native_enum_save_readbacks: 5, save_boundary: "synthetic onSave; no server files written",
    browser_errors: errors };
}

Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  console.error = originalError;
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error: unknown) => ({ status: "failed", modules, error: String(error instanceof Error ? error.stack : error),
  browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
