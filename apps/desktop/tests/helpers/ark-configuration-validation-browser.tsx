import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { I18nProvider } from "../../src/i18n";
import type { InstanceDetails } from "../../src/types";
import { ArkCommitInput } from "../../src/views/settings/ArkEditorFrame";
import { ArkLevelsEditor, ArkEngramPointsEditor } from "../../src/views/settings/ArkLevelsEditor";
import { ArkRuleNodeEditor } from "../../src/views/settings/ArkRuleNodeEditor";
import { parseArkRule } from "../../src/views/settings/ark-native-ast";
import type { SettingsObject } from "../../src/views/settings/settings-schema";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";
import "../../src/views/settings/ark-editors.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100dvh;display:grid;grid-template-rows:minmax(0,1fr) 40px;padding:16px;gap:12px";
const root = createRoot(fixture);
const store = createActivityNoticeStore();
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const moduleDetails = buildMockModuleDetails("arksurvivalascended");
const details: InstanceDetails = {
  summary: { id: "ark-validation", module_id: "arksurvivalascended", name: "ARK validation", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
  settings_json: "{}", ports: [], config_file_path: "C:/synthetic/ark/settings.json", saves_path: "C:/synthetic/ark/saves",
  backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5
};
let checks = 0, generation = 0, patches = 0;
let mode: "levels" | "engrams" | "rule" | "commit" = "levels";
let settings: SettingsObject = {};
let editorDisabled = false;
let rule = '(Future=(Existing="keep"))';
let commitValue = "1", committed = "", commitFails = true;
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}`); return found;
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function click(target: HTMLElement) { await act(async () => target.click()); }
async function until(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(frame);
  }
}
async function fill(target: HTMLInputElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(target instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function pressEnter(target: HTMLElement) {
  await act(async () => {
    target.focus();
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native keyboard dispatch failed");
  });
}
function paint() {
  const props = { moduleDetails, details, settings, sectionId: "leveling", disabled: editorDisabled,
    onPatch(patch: Readonly<SettingsObject>) { patches++; settings = { ...settings, ...patch }; paint(); } };
  root.render(<StrictMode><I18nProvider><ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: "Close" }}>
    <main id="editor" data-scene={mode} style={{ overflow: "auto", minHeight: 0 }} key={generation}>
      {mode === "levels" ? <ArkLevelsEditor {...props} settingKey="level_experience_ramp_overrides" /> : null}
      {mode === "engrams" ? <ArkEngramPointsEditor {...props} settingKey="override_player_level_engram_points" /> : null}
      {mode === "rule" ? <ArkRuleNodeEditor group={parseArkRule(rule, "Rule")} text={rule} path="Rule 1" disabled={false}
        onChange={(next) => { patches++; rule = next; paint(); }} /> : null}
      {mode === "commit" ? <details id="commit-details" open><summary>Attribute</summary><ArkCommitInput value={commitValue} label="Health"
        numeric onCommit={(value) => { if (commitFails) throw new Error("Source changed"); committed = value; }} /></details> : null}
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span><ActivityBar store={store} entries={[]} /></footer>
  </ActivityNoticeTarget.Provider></I18nProvider></StrictMode>);
}
async function mount(next: typeof mode) {
  mode = next; editorDisabled = false; generation++;
  await act(async () => { paint(); await document.fonts.ready; });
  await until(() => fixture.querySelector("#editor")?.getAttribute("data-scene") === next, "ARK editor did not finish loading its locale");
  check(store.getSnapshot().length === 0, "A prior editor left a stale activity notice");
}
function verifyError(target: HTMLInputElement | HTMLTextAreaElement) {
  check(store.getSnapshot().length === 1 && store.getSnapshot()[0].tone === "error", "A local failure must publish one activity error");
  check(!element("#editor").querySelector(".shell-activity-notice, .ark-editor__error, [role=alert]"), "Inline error still occupies the editor");
  check(target.getAttribute("aria-invalid") === "true", "The failed control lost its red frame");
  const description = document.getElementById(target.getAttribute("aria-errormessage") ?? "");
  check(description?.textContent === store.getSnapshot()[0].text, "Accessible error does not describe the same failure");
  check(getComputedStyle(description).position === "absolute" && description.getBoundingClientRect().height <= 1, "Accessible error takes up a row");
}
async function jump(target: HTMLInputElement | HTMLTextAreaElement, disclosure: HTMLDetailsElement, disabled = false) {
  await click(disclosure.querySelector<HTMLElement>("summary")!);
  check(!disclosure.open, "The failed editor did not collapse");
  await pressEnter(element(".shell-activity-expand"));
  await pressEnter(element(`.shell-activity-panel [data-configuration-validation-target="${CSS.escape(target.id)}"]`));
  check(disclosure.open, "Activity action did not reopen the failed editor");
  check(document.activeElement === (disabled ? target.closest(".ark-editor") : target),
    disabled ? "A disabled draft must focus its editor container" : "Activity action did not focus the failed draft");
  check(!fixture.querySelector(".shell-activity-panel"), "Activity details covered the focused editor");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const [next, field, raw, invalid, valid] of [
    ["levels", "level_experience_ramp_overrides", "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=100)", "curve,level,xp\nplayer,1,200", "curve,level,xp\nplayer,2,200"],
    ["engrams", "override_player_level_engram_points", "0\n5", "level,points\n2,10\n3,-5", "level,points\n2,10"]
  ] as const) {
    settings = { [field]: raw }; await mount(next);
    const disclosure = element<HTMLDetailsElement>(".ark-editor > details");
    await click(disclosure.querySelector<HTMLElement>("summary")!);
    const csv = element<HTMLTextAreaElement>("textarea.ark-editor__csv");
    await fill(csv, invalid);
    const height = element(".ark-editor").getBoundingClientRect().height;
    const normalBorder = getComputedStyle(csv).borderColor;
    const writes = patches;
    await click(disclosure.querySelector<HTMLButtonElement>("button")!);
    verifyError(csv);
    check(Math.abs(element(".ark-editor").getBoundingClientRect().height - height) < 1, "Validation shifted the ARK editor layout");
    await until(() => getComputedStyle(csv).borderColor !== normalBorder, "CSV error border did not become visible");
    check(getComputedStyle(csv).borderColor !== normalBorder, "CSV failure did not retain a visible red border");
    check(csv.value === invalid && settings[field] === raw && patches === writes, "A failed CSV import lost its draft or partially changed settings");
    await jump(csv, disclosure);
    if (next === "levels") {
      const message = store.getSnapshot()[0].text;
      editorDisabled = true; await act(async () => paint());
      check(csv.disabled, "The running lock did not disable the retained CSV draft");
      await jump(csv, disclosure, true);
      check(csv.value === invalid && csv.getAttribute("aria-invalid") === "true" && store.getSnapshot()[0]?.text === message,
        "Disabled navigation cleared the draft or its error");
      editorDisabled = false; await act(async () => paint());
    }
    await fill(csv, valid); await click(disclosure.querySelector<HTMLButtonElement>("button")!);
    check(store.getSnapshot().length === 0 && csv.value === "" && patches === writes + 1, "A corrected CSV did not apply once and remove its error");
  }
  await mount("rule");
  const property = element<HTMLInputElement>('input[aria-label="Rule 1 / Future New property name"]');
  await fill(property, "Existing");
  await click(property.parentElement!.querySelector<HTMLButtonElement>("button")!);
  verifyError(property);
  check(rule === '(Future=(Existing="keep"))', "An invalid property changed the native rule");
  await jump(property, property.closest("details")!);
  await fill(property, "Extra"); await click(property.parentElement!.querySelector<HTMLButtonElement>("button")!);
  check(rule === '(Future=(Existing="keep",Extra=""))' && store.getSnapshot().length === 0, "Property correction did not preserve the native rule");

  await mount("commit");
  const input = element<HTMLInputElement>('input[aria-label="Health"]');
  await fill(input, "2.5"); await act(async () => { input.focus(); input.blur(); });
  verifyError(input);
  commitValue = "3"; await act(async () => paint());
  check(input.value === "2.5", "A background value refresh erased the failed local draft");
  await jump(input, element<HTMLDetailsElement>("#commit-details"));
  commitFails = false; await act(async () => input.blur());
  check(committed === "2.5" && store.getSnapshot().length === 0, "Retry failed to apply the retained draft and remove its notice");
  await act(frame);
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  // Leave the captured desktop on the validated CSV failure layout.
  settings = { level_experience_ramp_overrides: "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=100)" };
  mode = "levels"; editorDisabled = false; generation++;
  await act(async () => paint());
  const capturedDisclosure = element<HTMLDetailsElement>(".ark-editor > details");
  await click(capturedDisclosure.querySelector<HTMLElement>("summary")!);
  const capturedCsv = element<HTMLTextAreaElement>("textarea.ark-editor__csv");
  await fill(capturedCsv, "curve,level,xp\nplayer,1,200");
  await click(capturedDisclosure.querySelector<HTMLButtonElement>("button")!);
  await click(element(`.shell-activity-viewport [data-configuration-validation-target="${CSS.escape(capturedCsv.id)}"]`));
  await act(async () => {
    await Promise.all(capturedCsv.getAnimations().map((animation) => animation.finished));
    await frame();
  });
  return { status: "passed", checks, csv_editors: 2, browser_errors: errors };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => root.unmount());
  if (store.getSnapshot().length) throw new Error("ARK validation notices remain after unmount");
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
