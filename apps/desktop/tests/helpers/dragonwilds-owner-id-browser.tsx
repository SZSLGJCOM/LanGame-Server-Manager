import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/runescapedragonwilds/schema.json";
import { I18nProvider } from "../../src/i18n";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import { InstanceSettingsDraftInvalidError } from "../../src/views/settings/instance-settings-save-queue";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { ConfigurationActivityHost } from "./configuration-activity-host";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const errors: string[] = [];
const saves: UpdateInstanceInput[] = [];
const cases: Array<{ name: string; code_units: number; accepted: boolean; input_complete: boolean; saved: boolean }> = [];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));

const details: InstanceDetails = {
  summary: { id: "dragonwilds-owner-ui", module_id: "runescapedragonwilds", name: "Owner ID fixture", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
  settings_json: JSON.stringify({ owner_id: "", server_name: "Owner fixture", default_world_name: "Retained",
    world_password: "fixture-password", future_setting: { retained: true } }),
  ports: [], config_file_path: "", saves_path: "", backup_uses_declared_saves_path: true,
  auto_backup_on_stop: false, backup_retention_count: 3, active_run: null
};
const moduleDetails: ModuleDetails = {
  summary: { id: "runescapedragonwilds", name: "Dragonwilds", version: "1.0", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify(schema), default_ports: [], runtime: {}
};
// Only the native read and persistence boundary are synthetic. The workspace,
// schema, field editing, validation, save queue, start barrier and CSS are real.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string) => {
  if (command !== "read_dragonwilds_world_settings") throw new Error(`Unexpected native command ${command}`);
  return { instance_id: details.summary.id, status: "empty", world_file: null, world_name: null,
    world_mode: null, revision: null, values: {}, overrides: {}, definitions: [], writable: false,
    message: null, backup_id: null };
} } });
let flushSettings: (() => Promise<void>) | undefined;
function Workspace() {
  const coordinator = useInstanceSettingsSaveCoordinator();
  flushSettings = () => coordinator.flush(details.summary.id);
  return <ConfigurationWorkspace details={details} moduleDetails={moduleDetails}
    bindAddressCandidates={[]} runtime={null} launchPlan={null} launchPlanError={null}
    onSave={async (input) => {
      saves.push(structuredClone(input));
      return { ...details, settings_json: input.settings_json };
    }} />;
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends Element>(selector: string): T {
  const target = fixture!.querySelector<T>(selector);
  check(target, `Missing ${selector}; ${fixture!.textContent?.slice(0, 800)}`);
  return target;
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function until(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(frame);
  }
}
function ownerInput() { return element<HTMLInputElement>('[data-field-key="owner_id"] input'); }
async function editAndCheck(name: string, value: string, accepted: boolean) {
  const input = ownerInput();
  const before = saves.length;
  check(!input.hasAttribute("maxlength"), "Native identity limits must not add HTML maxlength");
  await act(async () => {
    input.focus();
    input.setSelectionRange(0, input.value.length);
    // Use the browser's editing operation, preserving selection replacement and
    // real input events instead of bypassing the control with a property setter.
    check(document.execCommand(value ? "insertText" : "delete", false, value), "Browser text insertion failed");
  });
  check(ownerInput().value === value, `${name}: the complete pasted identity must remain visible`);
  if (accepted) {
    await until(() => ownerInput().getAttribute("aria-invalid") !== "true", `${name}: validation did not clear`);
    await act(async () => { check(flushSettings, "Settings coordinator is missing"); await flushSettings(); });
    check(saves.length === before + 1, `${name}: valid input must save exactly once`);
    check(JSON.parse(saves.at(-1)!.settings_json).owner_id === value, `${name}: save must preserve the complete identity`);
    check(JSON.parse(saves.at(-1)!.settings_json).future_setting.retained === true, "Unrelated settings changed");
  } else {
    await until(() => ownerInput().getAttribute("aria-invalid") === "true", `${name}: native length error did not appear`);
    const errorId = ownerInput().getAttribute("aria-errormessage");
    check(errorId && document.getElementById(errorId)?.textContent?.includes("32"),
      `${name}: the field's accessible error must explain the game limit`);
    check(element<HTMLElement>(".shell-activity-viewport .shell-activity-notice.is-error").textContent?.includes("32"),
      `${name}: the activity notice must explain the game limit`);
    let rejection: unknown;
    await act(async () => {
      try { check(flushSettings, "Settings coordinator is missing"); await flushSettings(); }
      catch (error) { rejection = error; }
    });
    check(rejection instanceof InstanceSettingsDraftInvalidError, `${name}: start/save barrier must reject the invalid draft`);
    check(saves.length === before, `${name}: invalid identity must not reach persistence`);
  }
  cases.push({ name, code_units: value.length, accepted, input_complete: ownerInput().value === value, saved: saves.length > before });
}
async function run() {
  await act(async () => {
    await prepareBrowserLocaleCatalogs();
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
      <ConfigurationActivityHost>
        <main className="server-detail-panel" style={{ height: "calc(100% - 32px)", margin: 16 }}><Workspace /></main>
      </ConfigurationActivityHost>
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  });
  await until(() => Boolean(fixture!.querySelector('[data-configuration-section-id="access"] > button')), "Configuration did not mount");
  await act(async () => { element<HTMLButtonElement>('[data-configuration-section-id="access"] > button').click(); });
  await until(() => Boolean(fixture!.querySelector('[data-field-key="owner_id"] input')), "OwnerId editor did not mount");
  check(saves.length === 0, "Opening the editor must not save settings");
  await editAndCheck("33 ASCII rejected", "a".repeat(33), false);
  await editAndCheck("32 ASCII accepted", "a".repeat(32), true);
  await editAndCheck("17 supplementary characters rejected", "😀".repeat(17), false);
  await editAndCheck("16 supplementary characters accepted", "😀".repeat(16), true);
  await editAndCheck("empty OwnerId accepted", "", true);
  await editAndCheck("complete invalid draft retained", "😀".repeat(17), false);
  // The production auto-save delay is 650 ms. Leave the final invalid draft
  // mounted beyond that boundary as well as exercising its explicit flush.
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 750)); await document.fonts.ready; await frame(); });
  check(saves.length === 3, "Invalid input must not schedule a delayed save");
  check(ownerInput().value === "😀".repeat(17), "A rejected draft must remain complete after the auto-save interval");
  await act(async () => { ownerInput().blur(); await frame(); });
  await until(() => !document.querySelector(".configuration-field-help-tooltip"), "Field help did not close before the screenshot");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", cases, accepted_saves: saves.length, browser_errors: errors };
}
run().catch((error) => ({ status: "failed", error: String(error), cases, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
