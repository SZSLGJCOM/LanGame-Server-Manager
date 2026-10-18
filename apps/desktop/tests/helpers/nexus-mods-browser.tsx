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
let details: InstanceDetails, epoch = 0, archived = false, installed = false;
let localImports = 0, inventoryReads = 0;
const openedLinks: string[] = [], commands: string[] = [];
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
async function tab(label: string) {
  const node = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === label);
  check(node, `Found ${label} workspace tab`); await act(async () => node.click());
}
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
  await settle(() => Boolean(fixture.querySelector(".mw-source-open-btn")), "Nexus source entry did not mount");
}
function header() {
  check(!fixture.querySelector(".mw-reference-row,[data-nexus-auth-open],[data-nexus-credentials],[data-nexus-file-review]"),
    "Nexus exposes no API key, ID/link installation or file-selection controls");
  check(!fixture.textContent?.includes("API Key") && !fixture.textContent?.includes("Nexus authorization"),
    "Unavailable authorization is absent from the product copy");
  const source = element(".mw-source-open-btn").getBoundingClientRect();
  check(source.width > 0 && source.height > 0 && source.left >= 0 && source.right <= innerWidth,
    "Nexus website action is reachable within the viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Nexus workspace has no horizontal overflow");
}
async function dropFile(target = element(".mw-dropzone")) {
  const file = new File(["isolated package"], "fixture.zip");
  Object.defineProperty(file, "path", { value: "fixture://nexus/package.zip" });
  const transfer = new DataTransfer(); transfer.items.add(file);
  Object.defineProperty(transfer, "effectAllowed", { value: "copy" });
  await act(async () => target.dispatchEvent(new DragEvent("drop", {
    bubbles: true, cancelable: true, dataTransfer: transfer })));
}
async function dispatch(command: string, payload: unknown) {
  commands.push(command);
  check(!command.startsWith("nexus_") && !command.includes("nexus_mod"),
    "No Nexus credential, metadata or download command is issued");
  switch (command) {
    case "read_manual_mod_inventory": inventoryReads++; return inventory();
    case "read_background_jobs": return [];
    case "read_instance_details_from_storage": return details;
    case "open_external_url": {
      const url = (payload as { url: string }).url;
      check(url === moduleDetails.mods?.source?.url, "System browser receives the declared Nexus game website");
      openedLinks.push(url); return;
    }
    case "stage_manual_mod_files": {
      const args = payload as { instanceId: string; sourcePaths: string[] };
      check(args.instanceId === details.summary.id && args.sourcePaths.length === 1
        && args.sourcePaths[0] === "fixture://nexus/package.zip", "Local import uses this instance and the dropped file");
      localImports++; installed = true; return stage();
    }
    default: throw new Error(`Unexpected Nexus fixture command: ${command}`);
  }
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-nexus", module_id: "sevendaystodie", name: "Nexus fixture" }, settings_json: "{}", active_run: null };
  mockWindows("nexus-fixture");
  mockIPC(dispatch, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount(); header();
  check(fixture.textContent?.includes("Download server-compatible files from the Nexus Mods website"),
    "Store explains the website download and local import workflow");
  await click(".mw-source-open-btn");
  check(openedLinks.length === 1, "Nexus website opens from the Store");
  const reads = inventoryReads;
  await dropFile();
  await settle(() => localImports === 1 && inventoryReads > reads && Boolean(fixture.querySelector(".mw-manual-success-bar")),
    "Local import did not refresh inventory and display its result");
  await tab("My Mods"); header();
  await settle(() => fixture.textContent?.includes("FixtureNexusMod") === true, "Imported local Mod did not appear in My Mods");
  check(details.settings_json === "{}", "Local file management does not create Nexus credentials or references in settings");
  for (const options of [{ running: true }, { archived: true }]) {
    await mount(options); header();
    const writes = localImports;
    if (options.archived) {
      const store = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === "Store");
      check(store?.disabled && !fixture.querySelector(".mw-dropzone"), "Archived instances expose no installation workspace");
      await dropFile(fixture);
    } else {
      check(element(".mw-dropzone").getAttribute("aria-disabled") === "true", "Running instances disable local import");
      await dropFile();
    }
    check(localImports === writes, "Disabled local import performs no mutation");
  }
  const originalFetch = window.fetch;
  window.fetch = async (request, options) => {
    if (request === "/__langame/api") {
      const { command, args } = JSON.parse(String(options?.body)) as { command: string; args: unknown };
      const value = await dispatch(command, args);
      return new Response(JSON.stringify({ ok: true, value }), { headers: { "Content-Type": "application/json" } });
    }
    return originalFetch(request, options);
  };
  Object.assign(window, { isTauri: false }); sessionStorage.setItem("langameLanToken", "isolated-fixture-token");
  try {
    await mount(); header();
    await dropFile();
    await settle(() => localImports === 2 && Boolean(fixture.querySelector(".mw-manual-success-bar")), "LAN local import did not complete");
    check(fixture.textContent?.includes("files installed"), "LAN keeps local file installation feedback");
  } finally {
    window.fetch = originalFetch; sessionStorage.removeItem("langameLanToken"); Object.assign(window, { isTauri: true });
  }
  await mount(); header();
  check(commands.every((command) => !command.startsWith("nexus_") && !command.includes("nexus_mod")),
    "Desktop and LAN workflows issue no Nexus API operations");
  check(errors.length === 0, "Nexus browser has no console or uncaught errors");
  return { status: "passed", checks, local_imports: localImports,
    nexus_api_calls: commands.filter((command) => command.startsWith("nexus_") || command.includes("nexus_mod")).length,
    browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch((error) => ({ status: "failed", error: String(error), checks, browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
