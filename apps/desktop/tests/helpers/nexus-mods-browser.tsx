import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { I18nProvider } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManualModStageResult } from "../../src/types";
import type { InstanceArchiveDetails } from "../../src/storage-management-types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US"); document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:grid;grid-template-rows:minmax(0,1fr) auto";
const checks: string[] = [], errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const consoleError = console.error; console.error = (...args) => { errors.push(args.map(String).join(" ")); consoleError(...args); };
const secret = "fixture-client-secret", signed = "nxm://7daystodie/mods/123/files/11?key=AbC%2F123&expires=9999999999&user_id=42";
let details: InstanceDetails, epoch = 0, configured = false, archived = false, installed = false;
let credentialWrites = 0, installs = 0, inventoryReads = 0, failStore = true, failInstall = true;
let failStatus = false;
const openedLinks: string[] = [];
let holdFirstStatus = true, releaseOldStatus: (() => void) | null = null;
let reviewMode: "multiple" | "unsupported" | "wrong-game" = "multiple";
let pendingInstall: Promise<void> | null = null, lastReviewed: string[] = [], lastSelections: { reference: string; file_id: string }[] = [];
const moduleDetails = buildMockModuleDetails("sevendaystodie");
function check(condition: unknown, label: string): asserts condition { if (!condition) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = document.querySelector<T>(selector); check(node, `Found ${selector}`); return node;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) { if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`); await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); }); }
}
async function click(selector: string) {
  const node = element(selector); node.scrollIntoView({ block: "nearest" });
  check(!(node instanceof HTMLButtonElement || node instanceof HTMLInputElement) || !node.disabled, `${selector} is enabled`);
  await act(async () => { node.focus(); node.click(); });
}
async function change(selector: string, value: string) {
  await act(async () => { const node = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true })); node.focus(); });
}
async function key(value: "Enter" | "Escape") {
  await act(async () => { const response = await fetch(`/__reliability_key/${new URLSearchParams(location.search).get("nonce")}/${value}`, { method: "POST" }); check(response.ok, `Native ${value}`); });
}
async function tab(label: string) {
  const node = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === label);
  check(node, `Found ${label} workspace tab`); await act(async () => node.click());
}
const input = () => element<HTMLInputElement>(".mw-reference-row input"), picker = () => Boolean(document.querySelector("[data-nexus-file-review]"));
function inventory() { return { instance_id: details.summary.id, module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods",
  target_path: "fixture://nexus/Mods", target_exists: installed, items: installed ? [{ name: "FixtureNexusMod", path: "fixture://nexus/Mods/FixtureNexusMod",
    item_type: "directory", inferred_id: null, file_count: 2, total_bytes: 4096 }] : [] }; }
function stage(): ManualModStageResult { return { instance_id: details.summary.id, module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods",
  target_path: "fixture://nexus/Mods", affected_root_names: ["FixtureNexusMod"], items: [], copied_file_count: 2, copied_total_bytes: 4096 }; }
function Harness() {
  const [notice, setNotice] = useState<HTMLDivElement | null>(null);
  const archive: InstanceArchiveDetails | undefined = archived ? { archive_id: "fixture-nexus-archive", instance: details,
    maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null },
    runs: { entries: [], total: 0, truncated: false }, log: { relative_path: null, text: "", truncated: false, issues: [] }, backups: { entries: [], issues: [], truncated: false } } : undefined;
  return <ActivityNoticeTarget.Provider value={{ element: notice, dismissLabel: "Close" }}><div style={{ minHeight: 0 }}>
    <ModWorkbench details={details} moduleDetails={moduleDetails} archive={archive} launchPlan={null}
      onSaveSettings={async () => { throw new Error("Nexus references and credentials must never be saved as server settings"); }} />
  </div><footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span><div ref={setNotice} className="shell-activity-notices" /></footer></ActivityNoticeTarget.Provider>;
}
async function mount(options: { running?: boolean; archived?: boolean } = {}) {
  archived = Boolean(options.archived); epoch++;
  details = { ...details, summary: { ...details.summary, status: options.running ? "Running" : "Stopped", active_process_count: options.running ? 1 : 0 } };
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Harness /></InstanceSettingsSaveProvider></I18nProvider>));
  await settle(() => Boolean(fixture.querySelector(".mw-reference-row input")), "Nexus header did not mount");
}
function header() {
  const field = input(), toolbar = element(".mw-manual-toolbar"), actions = element(".mw-manual-toolbar-actions");
  const a = field.getBoundingClientRect(), b = actions.getBoundingClientRect(), c = toolbar.getBoundingClientRect();
  check(fixture.querySelectorAll(".mw-reference-row input").length === 1 && a.width >= 160 && a.right < b.left,
    "Nexus has one usable header input left of its actions");
  check(a.top >= c.top && a.bottom <= c.bottom + 1 && Math.abs(a.top + a.height / 2 - b.top - b.height / 2) <= 1,
    "Nexus header controls share one baseline without a second strip");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Nexus workspace has no horizontal overflow");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-nexus", module_id: "sevendaystodie", name: "Nexus fixture" }, settings_json: "{}", active_run: null };
  mockWindows("nexus-fixture");
  mockIPC(async (command, payload) => {
    const args = payload as { apiKey?: string; references?: string[]; instanceId?: string; selections?: { reference: string; file_id: string }[] };
    switch (command) {
      case "nexus_account_status":
        if (holdFirstStatus) {
          holdFirstStatus = false;
          await new Promise<void>((resolve) => { releaseOldStatus = resolve; });
          return { configured: true, user_name: "Earlier account snapshot", is_premium: false };
        }
        if (failStatus) { failStatus = false; throw JSON.stringify({ code: "nexus_auth_invalid", message: secret }); }
        return { configured, user_name: configured ? "Fixture User" : null, is_premium: false };
      case "open_external_url":
        check(payload && typeof payload === "object" && "url" in payload && typeof payload.url === "string", "System browser receives an explicit public link");
        openedLinks.push(payload.url); return;
      case "nexus_store_api_key":
        check(args.apiKey === secret, "API key reaches only its dedicated native credential command"); credentialWrites++;
        if (failStore) { failStore = false; throw JSON.stringify({ code: "nexus_auth_invalid", message: secret }); }
        configured = true; return { configured, user_name: "Fixture User", is_premium: false };
      case "nexus_clear_api_key": configured = false; return { configured, user_name: null, is_premium: false };
      case "review_nexus_mod_references":
        check(configured && args.instanceId === details.summary.id && args.references?.length, "Nexus review uses the current instance and stored account");
        lastReviewed = args.references;
        if (reviewMode === "wrong-game") throw JSON.stringify({ code: "nexus_game_mismatch", message: signed });
        return args.references.map((reference, reference_index) => ({ reference_index, game_domain: "7daystodie", mod_id: String(123 + reference_index),
          mod_name: `Fixture server Mod ${reference_index + 1}`, page_url: `https://www.nexusmods.com/7daystodie/mods/${123 + reference_index}`,
          selected_file_id: reference.startsWith("nxm:") ? "11" : null,
          files: [
            { file_id: "10", name: "Server stable", file_name: "server-stable.zip", category: "MAIN", version: "1.0", size_bytes: 4096,
              description: "For the dedicated server. Check the author's dependency requirements.", downloadable: reviewMode !== "unsupported", unavailable_reason: reviewMode === "unsupported" ? "Package layout is not supported for this server." : null },
            { file_id: "11", name: "Server alternative", file_name: "server-alt.zip", category: "OPTIONAL", version: "1.1", size_bytes: 8192,
              description: "Alternative dedicated server files; explicit selection required.", downloadable: reviewMode !== "unsupported", unavailable_reason: reviewMode === "unsupported" ? "Unsupported archive format." : null },
            { file_id: "12", name: "Client executable", file_name: "client.exe", category: "MISC", version: "1.0", size_bytes: 1000,
              description: "Not an installable server archive.", downloadable: false, unavailable_reason: "Unsupported file format: exe" }
          ] })).reverse();
      case "install_nexus_mod_files":
        check(args.instanceId === details.summary.id && args.selections?.length, "Install uses current-instance selections rather than a client download URL");
        lastSelections = args.selections; installs++;
        if (failInstall) { failInstall = false; throw JSON.stringify({ code: "nexus_installation_failed", message: "Existing unowned Mods/Fixture/config.xml would be overwritten" }); }
        if (pendingInstall) await pendingInstall;
        installed = true; return stage();
      case "read_manual_mod_inventory": inventoryReads++; return inventory();
      case "read_background_jobs": return [];
      case "read_instance_details_from_storage": return structuredClone(details);
      default: throw new Error(`Unexpected Nexus fixture command: ${command}`);
    }
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true }); await mount(); header();
  const sameInput = input(); await change(".mw-reference-row input", "123"); await tab("My Mods"); header();
  check(input() === sameInput && input().value === "123", "Nexus My Mods retains the same header input and short ID");
  await tab("Store"); check(input() === sameInput && input().value === "123", "Returning to Store keeps the reference draft");
  await key("Enter"); await settle(() => Boolean(document.querySelector("[data-nexus-credentials]")), "Missing authorization did not expose the key entry");
  check(installs === 0 && lastReviewed.length === 0 && input().value === "123", "Missing API key cannot review or install and preserves input");
  await change("[data-nexus-api-key]", secret); await click("[data-nexus-key-save]");
  await settle(() => Boolean(document.querySelector('[data-nexus-credentials] [role="alert"]')), "Credential failure did not render");
  check(element<HTMLInputElement>("[data-nexus-api-key]").type === "password" && element<HTMLInputElement>("[data-nexus-api-key]").value === secret,
    "Failed credential verification preserves a masked retry draft");
  check(!document.body.textContent?.includes(secret), "Credential errors never echo the private input");
  await click("[data-nexus-key-save]"); await settle(() => element<HTMLInputElement>("[data-nexus-api-key]").value === "" && configured, "Credential retry did not save");
  await click("[data-nexus-credentials] a");
  check(openedLinks.at(-1) === "https://www.nexusmods.com/users/myaccount?tab=api", "API key application uses the official link in the system browser");
  await click("[data-nexus-key-close]"); failStatus = true; await click("[data-nexus-auth-open]");
  await settle(() => document.body.textContent?.includes("status could not be read") ?? false, "Revoked account status did not leave loading state");
  check(!element<HTMLButtonElement>("[data-nexus-key-clear]").disabled, "Unknown or revoked authorization can still be removed from local credentials");
  await click("[data-nexus-key-clear]"); await settle(() => !configured && element<HTMLButtonElement>("[data-nexus-key-clear]").disabled, "Credential clear did not refresh state");
  const releaseHeaderRead = releaseOldStatus;
  check(releaseHeaderRead, "The older header credential read remains in flight");
  await act(async () => releaseHeaderRead());
  check(element("[data-nexus-auth-open]").getAttribute("data-nexus-authorized") === "false", "A late header account read cannot overwrite a newer credential removal");
  await change("[data-nexus-api-key]", secret); await click("[data-nexus-key-close]"); await click("[data-nexus-auth-open]");
  await settle(() => !element<HTMLInputElement>("[data-nexus-api-key]").disabled, "Reopened authorization did not finish reading");
  check(element<HTMLInputElement>("[data-nexus-api-key]").value === "", "Closing authorization discards its password draft");
  await change("[data-nexus-api-key]", secret); await click("[data-nexus-key-save]");
  await settle(() => configured && element<HTMLInputElement>("[data-nexus-api-key]").value === "", "Authorization did not save again");
  await click("[data-nexus-key-close]"); await click(".mw-reference-row button"); await settle(picker, "Review did not open file selection");
  check(lastReviewed[0] === "123" && element<HTMLButtonElement>("[data-nexus-install]").disabled && installs === 0, "Short ID preview never automatically chooses a file");
  check(element<HTMLInputElement>('[data-nexus-file-id="12"]').disabled && document.body.textContent?.includes("Unsupported file format: exe"),
    "Unavailable files expose their reason and cannot be selected");
  await click("[data-nexus-file-review] a");
  check(openedLinks.at(-1) === "https://www.nexusmods.com/7daystodie/mods/123", "File review opens only the public author page in the system browser");
  await click("[data-nexus-cancel]"); check(input().value === "123" && installs === 0, "Cancelling file selection retains input without installing");
  reviewMode = "unsupported"; await click(".mw-reference-row button"); await settle(picker, "Unsupported review did not open");
  check(element<HTMLButtonElement>("[data-nexus-install]").disabled && document.body.textContent?.includes("Package layout is not supported"), "Unsupported packages remain reviewable without an install action");
  await click("[data-nexus-cancel]"); reviewMode = "wrong-game"; await click(".mw-reference-row button");
  await settle(() => fixture.textContent?.includes("belongs to another game") ?? false, "Wrong-game rejection did not render");
  check(input().value === "123" && !document.body.textContent?.includes("key=AbC"), "Wrong-game errors retain input and do not echo signed download material");
  reviewMode = "multiple"; await change(".mw-reference-row input", signed); await key("Enter"); await settle(picker, "Signed nxm link did not reach review");
  check(lastReviewed[0] === signed && !element<HTMLButtonElement>("[data-nexus-install]").disabled, "Signed nxm query bytes are preserved and its explicit file is reviewed");
  await click("[data-nexus-cancel]"); await tab("My Mods"); await change(".mw-reference-row input", "123 456"); await key("Enter"); await settle(picker, "Multiple project review did not open");
  await click('[data-nexus-reference-index="0"][data-nexus-file-id="11"]');
  await click('[data-nexus-reference-index="1"][data-nexus-file-id="10"]'); await click("[data-nexus-install]");
  await settle(() => document.body.textContent?.includes("Existing unowned Mods/Fixture/config.xml") ?? false, "Install file-conflict context did not render");
  check(input().value === "123 456" && !installed && picker(), "Install failure keeps the input and explicit file selections for retry");
  check(lastSelections.some((item) => item.reference === "123" && item.file_id === "11") && lastSelections.some((item) => item.reference === "456" && item.file_id === "10"),
    "Reference indices map reordered reviews back to the original in-memory references");
  let finish!: () => void; pendingInstall = new Promise((resolve) => { finish = resolve; });
  const reads = inventoryReads; await click("[data-nexus-install]");
  await settle(() => element<HTMLButtonElement>("[data-nexus-cancel]").disabled, "Pending install did not lock file selection");
  check(element<HTMLButtonElement>("[data-nexus-install]").disabled && element<HTMLInputElement>('[data-nexus-file-id="10"]').disabled, "Busy installation disables duplicate submission and selection changes");
  await key("Escape"); check(picker(), "Busy installation keeps its modal open on Escape");
  await act(async () => finish()); pendingInstall = null;
  await settle(() => !picker() && input().value === "" && inventoryReads > reads && fixture.textContent?.includes("FixtureNexusMod") === true, "Successful installation did not refresh My Mods");
  check(details.settings_json === "{}" && !JSON.stringify(Object.keys(localStorage).map((key) => localStorage.getItem(key))).includes("key=AbC"), "Signed references and credentials never enter server settings or browser storage");
  await mount({ running: true }); check(input().disabled && element<HTMLButtonElement>("[data-nexus-auth-open]").disabled, "Running instances cannot install or change Nexus authorization through the workbench");
  await mount({ archived: true }); check(input().disabled && element<HTMLButtonElement>("[data-nexus-auth-open]").disabled, "Read-only archive keeps its persistent Nexus header disabled");
  const originalFetch = window.fetch, lanCommands: string[] = [];
  window.fetch = async (request, options) => {
    if (typeof request === "string" && request === "/__langame/api") {
      const payload: unknown = JSON.parse(String(options?.body));
      check(payload && typeof payload === "object" && "command" in payload && typeof payload.command === "string", "LAN fixture receives a structured management command");
      lanCommands.push(payload.command);
      const value = payload.command === "read_manual_mod_inventory" ? inventory() : payload.command === "read_background_jobs" ? []
        : payload.command === "stage_manual_mod_files" ? stage() : null;
      return new Response(JSON.stringify({ ok: true, value }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return originalFetch(request, options);
  };
  Object.assign(window, { isTauri: false }); sessionStorage.setItem("langameLanToken", "isolated-fixture-token");
  try {
    await mount();
    check(input().disabled && element<HTMLButtonElement>("[data-nexus-auth-open]").disabled
      && fixture.textContent?.includes("LAN management page does not accept API keys"), "LAN users see a disabled Nexus input and actionable desktop-host guidance");
    const file = new File(["isolated package"], "fixture.zip"); Object.defineProperty(file, "path", { value: "fixture://nexus/package.zip" });
    const transfer = new DataTransfer(); transfer.items.add(file); Object.defineProperty(transfer, "effectAllowed", { value: "copy" });
    await act(async () => element(".mw-dropzone").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer })));
    await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "LAN local file import feedback was hidden by the Nexus-only notice");
    check(fixture.textContent?.includes("LAN management page does not accept API keys") && fixture.textContent?.includes("files installed"),
      "LAN Nexus guidance coexists with the existing local-file installation result");
    check(!lanCommands.some((command) => command.startsWith("nexus_") || command.includes("nexus_mod")), "No Nexus credential, review or installation command crosses LAN transport");
  } finally {
    window.fetch = originalFetch; sessionStorage.removeItem("langameLanToken"); Object.assign(window, { isTauri: true });
  }
  await mount(); header(); await change(".mw-reference-row input", "123"); await tab("My Mods"); await click(".mw-reference-row button"); await settle(picker, "Final file review did not open");
  const modal = element<HTMLDialogElement>(".nexus-dialog"), bounds = modal.getBoundingClientRect(), footer = element(".nexus-dialog__footer").getBoundingClientRect();
  check(bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight && footer.bottom <= innerHeight, "Nexus file review and installation footer fit the desktop viewport");
  check(errors.length === 0, "Nexus browser has no console or uncaught errors");
  return { status: "passed", checks, installs, credential_writes: credentialWrites, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch((error) => ({ status: "failed", error: String(error), checks, browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
