import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { AsaModConfiguration } from "../../src/views/servers/AsaModConfiguration";
import { ASA_MOD_AUTHOR_CATALOG, type AsaModIniFile } from "../../src/views/servers/asa-mod-configuration";
import type { SettingsObject } from "../../src/views/settings/settings-schema";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;overflow:auto;max-width:980px;margin:auto";
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [];
const nonce = new URLSearchParams(location.search).get("nonce");
const known = "1254808", unknown = "928708";
// The production author catalog/model/editor run here; only persistence is substituted.
const authorDefinitions = ASA_MOD_AUTHOR_CATALOG;
let stored: SettingsObject = { mod_ids_csv: `${known},${unknown}`, passive_mod_ids_csv: "500", unrelated: "retained",
  asa_mod_configurations: {
    [known]: { game_ini: "", game_user_settings_ini: "[BetterVariants]\nDebug=False\n" },
    [unknown]: { game_ini: "[OtherFixture]\nSaved=original\n", game_user_settings_ini: "[OtherFile]\nKeep=1\n" },
  } };
let modId = known, instanceId = "component-fixture-a", disabled = false, readOnly = false;
let failSave = false, pendingSave: Promise<void> | null = null;
let heldSettings: SettingsObject | null = null, failReload = false, pendingReload: Promise<void> | null = null;
let capturedReload: SettingsObject | null = null, earlySaveAcknowledgement = false;
const saves: Array<{ next: SettingsObject; base: SettingsObject }> = [];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => root.unmount()); return { browser_errors: errors, native_dialogs: 0 };
} });
function assert(condition: unknown, label: string): asserts condition { if (!condition) throw new Error(label); checks.push(label); }
function element<T extends Element>(selector: string): T {
  const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node;
}
function configs() { return stored.asa_mod_configurations as Record<string, Record<AsaModIniFile, string>>; }
function draw() {
  root.render(<I18nProvider><main className="mw-selected-config-panel">
    <AsaModConfiguration instanceId={instanceId} modId={modId} settings={heldSettings ?? stored} disabled={disabled} readOnly={readOnly}
      onReload={async () => {
        const result = structuredClone(capturedReload ?? stored);
        if (pendingReload) await pendingReload;
        if (failReload) throw new Error("Controlled per-Mod reload failure");
        return result;
      }}
      authorDefinitions={authorDefinitions} installed onSave={async (next, base) => {
        assert(JSON.stringify(base) === JSON.stringify(stored), "Saving retains the complete reviewed persistence baseline");
        saves.push({ next: structuredClone(next), base: structuredClone(base) });
        const acknowledgement = earlySaveAcknowledgement ? structuredClone(next) : null;
        if (acknowledgement) stored = structuredClone(next);
        if (pendingSave) await pendingSave;
        if (failSave) throw new Error("Controlled per-Mod save failure");
        if (acknowledgement) return acknowledgement;
        stored = structuredClone(next); draw();
        return structuredClone(stored);
      }} />
  </main></I18nProvider>);
}
async function render() {
  await act(async () => draw());
  await settle(() => Boolean(fixture.querySelector(`[data-asa-mod-config="${modId}"] [data-asa-mod-file]`)));
}
async function settle(predicate: () => boolean) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error("Per-Mod configuration did not settle");
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function change(target: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    const prototype = target instanceof HTMLSelectElement ? HTMLSelectElement.prototype
      : target instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event(target instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
const input = () => element<HTMLTextAreaElement>(".asa-mod-config__input");
const save = () => element<HTMLButtonElement>("[data-asa-mod-save]");
async function choose(file: AsaModIniFile) { await change(element<HTMLSelectElement>("[data-asa-mod-file]"), file); }
async function switchMod(id: string) { modId = id; await render(); }
async function click(target: HTMLElement) { await act(async () => target.click()); }
function visibleBodyText() {
  const copy = element<HTMLElement>(".asa-mod-config").cloneNode(true) as HTMLElement;
  for (const help of copy.querySelectorAll(".configuration-field-help-a11y")) help.remove();
  return copy.textContent ?? "";
}
async function verifyHelp(anchor: HTMLElement, expected: string, label: string) {
  const description = document.getElementById(anchor.getAttribute("aria-describedby") ?? "");
  assert(description?.textContent?.includes(expected), `${label} retains its accessible instructions`);
  await act(async () => anchor.focus());
  await settle(() => Boolean(document.querySelector(".configuration-field-help-tooltip.is-visible")));
  const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")!;
  assert(bubble.textContent === description.textContent && bubble.getBoundingClientRect().left >= 0
    && bubble.getBoundingClientRect().right <= innerWidth, `${label} opens its complete instructions inside the viewport`);
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
    assert(response.ok, `${label} receives Escape`);
  });
  await settle(() => !document.querySelector(".configuration-field-help-tooltip"));
  assert(document.activeElement === anchor, `${label} closes without moving keyboard focus`);
  await act(async () => anchor.blur());
}
async function openPreview() {
  const preview = element<HTMLDetailsElement>(".asa-mod-config__preview");
  if (!preview.open) await click(preview.querySelector<HTMLElement>("summary")!);
  assert(preview.open && preview.querySelector("table")!.checkVisibility(), "Expanded preview visibly renders its reviewed rows");
}
async function importFile(file: File) {
  await act(async () => {
    const control = element<HTMLInputElement>('.asa-mod-config input[type="file"]');
    Object.defineProperty(control, "files", { configurable: true, value: [file] });
    control.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await settle(() => element(".asa-mod-config").getAttribute("aria-busy") === "false");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs); await render();
  assert(element<HTMLSelectElement>("[data-asa-mod-file]").value === "game_user_settings_ini", "Verified author fields choose their declared destination");
  assert(fixture.querySelectorAll(".asa-mod-config__field").length === 2, "Known Mod exposes only its two author-defined Boolean fields");
  const boolean = (key: string) => element<HTMLSelectElement>(`[data-asa-mod-parameter="${key}"] select`);
  assert(!fixture.querySelector("[data-asa-mod-clear], [data-asa-mod-reload], [data-asa-mod-destination], .asa-mod-config__dirty"),
    "Unchanged configuration has no redundant clear, reload, destination or unchanged status controls");
  const authorLink = element<HTMLButtonElement>(".asa-mod-config__author-link");
  assert(authorLink.parentElement?.classList.contains("asa-mod-config__heading") && authorLink.querySelector("svg")
    && !authorLink.textContent?.trim() && authorLink.getAttribute("aria-label") === (zh ? "作者文档" : "Author documentation"),
    "Author documentation is an accessible icon within the heading");
  const sharedInstructions = zh ? "配置应用于当前选定的地图范围" : "These settings apply to the selected map scope";
  assert(!visibleBodyText().includes(sharedInstructions) && !fixture.querySelector(".asa-mod-config__field code"),
    "General instructions and INI field identities do not occupy the configuration body");
  await verifyHelp(element<HTMLHeadingElement>(".asa-mod-config__heading h3"), sharedInstructions, "Configuration heading help");
  await verifyHelp(boolean("Debug"), "[BetterVariants] Debug", "Author parameter help");
  const preview = element<HTMLDetailsElement>(".asa-mod-config__preview"), initialText = input().value;
  assert(!preview.open && !preview.querySelector("table")!.checkVisibility(), "Preview starts collapsed with its duplicate table hidden");
  await openPreview(); await click(preview.querySelector<HTMLElement>("summary")!);
  assert(!preview.open && input().value === initialText && save().disabled, "Opening and closing the preview does not edit or save configuration");
  await change(boolean("DebugSpawns"), "True");
  assert(input().value.includes("Debug=False") && input().value.includes("DebugSpawns=True"), "Dependent field changes only its authoritative INI key");
  assert([...fixture.querySelectorAll('[role="status"]')].some((node) => node.textContent?.includes("DebugSpawns") && node.textContent.includes("Debug=True")),
    "DebugSpawns explains its Debug dependency without enabling Debug automatically");
  await click(save()); await settle(() => !save().disabled || !input().value.includes("Debug=False") || configs()[known].game_user_settings_ini.includes("DebugSpawns=True"));
  assert(configs()[known].game_user_settings_ini.includes("DebugSpawns=True"), "Author field saves its INI text");

  await choose("game_ini");
  const alpha = '[PrivateFixture]\nRepeat=one\nRepeat=two\nMarkup=<img src=x onerror="window.__asaIniExecuted=true">\n';
  await change(input(), alpha);
  await openPreview();
  assert(fixture.querySelectorAll(".asa-mod-config__preview tbody tr").length === 3, "Preview retains repeated keys in order");
  assert(!fixture.querySelector(".asa-mod-config__preview img, .asa-mod-config__preview script")
    && !(globalThis as { __asaIniExecuted?: boolean }).__asaIniExecuted, "INI values render as safe text without executing HTML");
  await choose("game_user_settings_ini");
  assert(input().value.includes("DebugSpawns=True"), "Switching files retains the other file's reviewed content");
  await switchMod(unknown); await change(input(), "[OtherFixture]\nUnsaved=beta\n");
  const unknownInstructions = zh ? "尚未适配" : "not been adapted";
  assert(!visibleBodyText().includes(unknownInstructions) && input(), "Unknown Mod keeps its editor without a permanent explanatory paragraph");
  await verifyHelp(element<HTMLHeadingElement>(".asa-mod-config__heading h3"), unknownInstructions, "Unknown Mod heading help");
  await switchMod(known); await choose("game_ini"); assert(input().value === alpha, "Switching Mods retains their independent file drafts");
  await click(save()); await settle(() => configs()[known].game_ini === alpha);
  assert(configs()[unknown].game_ini === "[OtherFixture]\nSaved=original\n" && stored.unrelated === "retained",
    "Saving one file does not persist another Mod's draft or alter other settings");
  await switchMod(unknown); assert(input().value.includes("Unsaved=beta"), "An unrelated successful save does not discard this Mod's draft");

  await importFile(new File([Uint8Array.from([0xc3, 0x28])], "bad.ini"));
  assert(input().value.includes("Unsaved=beta") && !save().disabled, "Invalid import keeps the previous editable draft");
  await importFile(new File([new Uint8Array(65537)], "too-big.ini"));
  assert(input().value.includes("Unsaved=beta"), "Oversized import does not replace the draft");
  const text = "[ImportedFixture]\nKey=中文\nKey=second\n", units = Uint16Array.from([0xfeff, ...Array.from(text, (character) => character.charCodeAt(0))]);
  await importFile(new File([units], "utf16.ini"));
  await openPreview();
  assert(input().value === text && fixture.querySelectorAll(".asa-mod-config__preview tbody tr").length === 2,
    "UTF-16 BOM import produces a reviewable complete ordered INI draft");

  let finishImport!: (value: ArrayBuffer) => void;
  const late = new File(["[LateFixture]\nKey=late\n"], "late.ini");
  Object.defineProperty(late, "arrayBuffer", { value: () => new Promise<ArrayBuffer>((resolve) => { finishImport = resolve; }) });
  await act(async () => {
    const control = element<HTMLInputElement>('.asa-mod-config input[type="file"]');
    Object.defineProperty(control, "files", { configurable: true, value: [late] });
    control.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await choose("game_user_settings_ini");
  await act(async () => finishImport(new TextEncoder().encode("[LateFixture]\nKey=late\n").buffer));
  assert(input().value === "[OtherFile]\nKeep=1\n", "A late file import cannot write to a newly selected destination");
  await choose("game_ini"); assert(input().value === text, "Cancelled import retains its previous Mod/file draft");

  failSave = true;
  await click(save()); await settle(() => fixture.textContent?.includes("Controlled per-Mod save failure") ?? false);
  assert(input().value === text && !save().disabled, "Async save failure retains the draft for retry");
  failSave = false; let release!: () => void;
  pendingSave = new Promise<void>((resolve) => { release = resolve; });
  await click(save()); await switchMod(known); await choose("game_ini");
  await change(input(), alpha.replace("Repeat=two", "Repeat=newer"));
  assert(save().disabled, "A different Mod cannot start a simultaneous full-settings save");
  await act(async () => release()); pendingSave = null;
  await settle(() => !save().disabled);
  assert(input().value.includes("Repeat=newer") && configs()[unknown].game_ini === text,
    "Late save completion targets its original Mod and preserves the newly selected draft");

  await switchMod(unknown); await change(input(), "");
  assert(input().value === "" && fixture.querySelector(".asa-mod-config__clear-review")?.textContent?.includes("Game.ini"),
    "Clearing is a reviewable draft action naming its destination file");
  assert(!fixture.querySelector(".asa-mod-config__preview") && save().textContent === (zh ? "移除配置" : "Remove configuration"),
    "An empty edited draft has no empty preview and its save action explicitly removes configuration");
  const otherFile = configs()[unknown].game_user_settings_ini;
  await click(save()); await settle(() => configs()[unknown].game_ini === "");
  assert(configs()[unknown].game_user_settings_ini === otherFile, "Saving removal leaves the other file unchanged");
  await change(input(), "[PrivateFixture]\nDraft=retain\n");
  const beforeLocked = saves.length;
  disabled = true; await render(); await click(save()); assert(save().disabled && saves.length === beforeLocked, "Busy/running controls cannot save");
  disabled = false; readOnly = true; await render(); await click(save()); assert(save().disabled && saves.length === beforeLocked, "Read-only archive controls cannot save");
  readOnly = false; await render();
  stored = { ...stored, asa_mod_configurations: { ...configs(), [unknown]: { ...configs()[unknown], game_ini: "[ExternalFixture]\nChanged=yes\n" } } };
  await render(); await click(save());
  await settle(() => fixture.textContent?.includes(zh ? "已经改变" : "changed") ?? false);
  assert(input().value.includes("Draft=retain") && saves.length === beforeLocked, "A changed saved baseline cannot overwrite current content");

  heldSettings = structuredClone(stored);
  stored = { ...stored, unrelated: "fresh-authority", asa_mod_configurations: { ...configs(),
    [unknown]: { ...configs()[unknown], game_ini: "[ReadbackFixture]\nFresh=yes\n" } } };
  await render();
  const reload = async () => {
    await click(element<HTMLButtonElement>("[data-asa-mod-reload]"));
    await click(element<HTMLButtonElement>(".inline-confirm-submit"));
    await settle(() => element(".asa-mod-config").getAttribute("aria-busy") === "false");
  };
  await reload();
  assert(input().value === "[ReadbackFixture]\nFresh=yes\n", "Authoritative reload replaces only the reviewed file without waiting for parent props");
  await change(input(), "[ReadbackFixture]\nFresh=first\n"); await click(save());
  await settle(() => configs()[unknown].game_ini.includes("Fresh=first"));
  await change(input(), "[ReadbackFixture]\nFresh=second\n"); await click(save());
  await settle(() => configs()[unknown].game_ini.includes("Fresh=second"));
  assert(stored.unrelated === "fresh-authority", "Immediate first and second saves use returned authority while parent props remain stale");
  await change(input(), "[ReadbackFixture]\nFresh=unsaved\n");
  failReload = true; const beforeFailedReload = input().value;
  await reload();
  assert(input().value === beforeFailedReload && fixture.textContent?.includes("Controlled per-Mod reload failure"), "Failed reload preserves the unsaved draft and its error");
  failReload = false;
  if (fixture.querySelector(".inline-confirm-submit")) await click(element<HTMLButtonElement>(".inline-confirm-submit"));
  else await reload();
  await settle(() => input().value.includes("Fresh=second"));
  await choose("game_user_settings_ini"); await change(input(), "[OtherFile]\nKeep=dirty-other-file\n");
  await choose("game_ini"); await change(input(), "[ReadbackFixture]\nFresh=discard-target\n");
  let finishReload!: () => void; pendingReload = new Promise<void>((resolve) => { finishReload = resolve; });
  await click(element<HTMLButtonElement>("[data-asa-mod-reload]")); await click(element<HTMLButtonElement>(".inline-confirm-submit"));
  await choose("game_user_settings_ini");
  await act(async () => finishReload()); pendingReload = null;
  await settle(() => element(".asa-mod-config").getAttribute("aria-busy") === "false");
  assert(input().value === "[OtherFile]\nKeep=dirty-other-file\n", "Late reload cannot discard another file's dirty draft after selection changes");
  await choose("game_ini"); assert(input().value.includes("Fresh=second"), "The completed readback resets only its originally reviewed destination");
  heldSettings = null;
  const beforeCleanParent = { unrelated: stored.unrelated, otherMod: structuredClone(configs()[known]),
    otherFile: configs()[unknown].game_user_settings_ini };
  stored = { ...stored, asa_mod_configurations: { ...configs(), [unknown]: { ...configs()[unknown], game_ini: "[NewPropsFixture]\nChanged=yes\n" } } };
  await render();
  assert(input().value === "[NewPropsFixture]\nChanged=yes\n" && save().disabled,
    "A clean editor follows new saved parent content without creating an unsaved draft");
  await change(input(), "[NewPropsFixture]\nChanged=local\n");
  const beforeNewPropsSave = saves.length; await click(save());
  await settle(() => configs()[unknown].game_ini === "[NewPropsFixture]\nChanged=local\n");
  assert(saves.length === beforeNewPropsSave + 1 && stored.unrelated === beforeCleanParent.unrelated
    && JSON.stringify(configs()[known]) === JSON.stringify(beforeCleanParent.otherMod)
    && configs()[unknown].game_user_settings_ini === beforeCleanParent.otherFile,
    "A new edit after clean parent refresh saves once against current authority and preserves unrelated Mod/file settings");

  await change(input(), "[DelayedReadDraft]\nKeep=local\n");
  const delayedReadDraft = input().value;
  capturedReload = structuredClone(stored);
  let releaseOldRead!: () => void; pendingReload = new Promise<void>((resolve) => { releaseOldRead = resolve; });
  await click(element<HTMLButtonElement>("[data-asa-mod-reload]")); await click(element<HTMLButtonElement>(".inline-confirm-submit"));
  stored = { ...stored, asa_mod_configurations: { ...configs(), [unknown]: { ...configs()[unknown], game_ini: "[InterleavedRead]\nValue=parent\n" } } };
  await render(); await act(async () => releaseOldRead()); pendingReload = null; capturedReload = null;
  await settle(() => element(".asa-mod-config").getAttribute("aria-busy") === "false");
  assert(input().value === delayedReadDraft && fixture.textContent?.includes(zh ? "已经改变" : "changed"),
    "An older read arriving after new parent contents retains the draft and requests a fresh read");
  if (fixture.querySelector(".inline-confirm-submit")) await click(element<HTMLButtonElement>(".inline-confirm-submit"));
  else await reload();
  await settle(() => input().value.includes("InterleavedRead"));

  await change(input(), "[DelayedSaveDraft]\nValue=saved\n");
  const delayedSaveDraft = input().value;
  let releaseOldSave!: () => void; pendingSave = new Promise<void>((resolve) => { releaseOldSave = resolve; }); earlySaveAcknowledgement = true;
  await click(save());
  stored = { ...stored, asa_mod_configurations: { ...configs(), [unknown]: { ...configs()[unknown], game_ini: "[InterleavedSave]\nValue=parent\n" } } };
  await render(); await act(async () => releaseOldSave()); pendingSave = null; earlySaveAcknowledgement = false;
  await settle(() => element(".asa-mod-config").getAttribute("aria-busy") === "false");
  assert(input().value === delayedSaveDraft && fixture.textContent?.includes(zh ? "保存已完成" : "Saving completed")
    && !fixture.querySelector('[role="alert"]'), "A late save acknowledgement reports completed persistence and preserves the draft without replacing newer parent contents");
  const beforeLateRetry = saves.length; await click(save());
  assert(saves.length === beforeLateRetry && fixture.textContent?.includes(zh ? "已经改变" : "changed"),
    "An old save acknowledgement cannot relabel its snapshot as the new parent baseline");

  instanceId = "component-fixture-b"; await render();
  assert(input().value === "[InterleavedSave]\nValue=parent\n" && save().disabled, "Switching instances drops the previous instance's isolated drafts");
  await switchMod(known); await choose("game_user_settings_ini");
  await change(boolean("Debug"), "True");
  assert(input().value.includes("Debug=True") && input().value.includes("DebugSpawns=True"), "Final adapted form and raw editor share one draft");
  for (const preview of fixture.querySelectorAll<HTMLDetailsElement>(".asa-mod-config__preview[open]")) {
    await click(preview.querySelector<HTMLElement>("summary")!);
  }
  await act(async () => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    fixture.scrollTop = 0;
  });
  await settle(() => !document.querySelector(".configuration-field-help-tooltip"));
  assert(fixture.scrollWidth <= fixture.clientWidth + 1, "Configuration editor stays inside the viewport");
  assert(errors.length === 0, "Browser emitted no errors");
  return { status: "passed", checks, saves: saves.length, browser_errors: errors, native_settings_saved: false, fixture: "component-persistence-boundary" };
}
void run().catch((error) => ({ status: "failed", error: error instanceof Error ? error.stack : String(error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
