import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { listInstanceArchives, restoreInstanceArchive } from "../../src/api-storage";
import { I18nProvider } from "../../src/i18n";
import { useSelectedInstanceModuleDetailsSync } from "../../src/hooks/useDesktopEffects";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails } from "../../src/types";
import type { InstanceArchiveList } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`${description}: ${fixture.textContent}`);
  checks.push(description);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => { resolve = accept; });
  return { promise, resolve };
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function waitForConfigurationResult() {
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector('[data-field-key="server_name"] input,[data-configuration-state="error"]')) {
    if (performance.now() >= deadline) throw new Error(`Configuration did not settle: ${fixture.textContent}`);
    await act(async () => { await frame(); });
  }
}
const moduleDetails: ModuleDetails = {
  summary: { id: "restored-fixture", name: "Restored game", version: "1.0.0", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify({ type: "object", properties: {
    server_name: { type: "string", title: "Server name", "x-lsgm-section": "network" }
  } }),
  default_ports: [{ name: "game", port: 25565, protocol: "tcp" }],
  install: { verification_path: "server.exe" },
  process: { executable: "server.exe" },
  runtime: { port_roles: [{ port_names: ["game"], role: "player" }] }
};
const details: InstanceDetails = {
  summary: { id: "restored-instance", module_id: moduleDetails.summary.id, name: "Retained world", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
  settings_json: JSON.stringify({ server_name: "Retained world" }),
  ports: [{ name: "game", port: 25566, protocol: "tcp" }],
  config_file_path: "C:/fixture/restored-instance/config/settings.json", saves_path: "C:/fixture/restored-instance/saves",
  backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5, active_run: null
};
const inventory = deferred<InstanceArchiveList>();
let restored = false;
let inventoryPending = false;
let moduleFailure: string | null = null;
let saveCalls = 0;
const moduleReads: Array<{ counts: unknown; inventoryPending: boolean }> = [];
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: Record<string, unknown> = {}) => {
  if (command === "restore_instance_archive") {
    restored = true;
    return Promise.resolve({ archive_id: "retained-archive", instance_id: details.summary.id, instance_name: details.summary.name,
      restored_instance_root: "C:/fixture/restored-instance", external_saves_restore_required: false,
      external_saves_backup_id: null, preserved_external_saves_path: null });
  }
  if (command === "list_instance_archives") {
    inventoryPending = true;
    return inventory.promise.finally(() => { inventoryPending = false; });
  }
  if (command === "read_module_details") {
    const counts = args.includePreservedProgramCounts ?? args.include_preserved_program_counts;
    moduleReads.push({ counts, inventoryPending });
    if (moduleFailure) return Promise.reject(new Error(moduleFailure));
    if (inventoryPending && counts !== false) return Promise.reject(new Error("archive inventory is busy"));
    return Promise.resolve(structuredClone(moduleDetails));
  }
  throw new Error(`Unexpected native command: ${command}`);
} } });

let requestRetry: () => void;
function RestoredConfiguration() {
  const [loaded, setLoaded] = useState<ModuleDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  requestRetry = () => setRetry((value) => value + 1);
  useSelectedInstanceModuleDetailsSync({ enabled: true, selectedModuleId: details.summary.module_id, retryGeneration: retry,
    onLoaded: (value, failure) => { setLoaded(value); setError(failure?.message ?? null); } });
  return <ConfigurationWorkspace details={details} moduleDetails={loaded} moduleDetailsError={error}
    onRetryModuleDetails={requestRetry} bindAddressCandidates={[]} runtime={null} launchPlan={null} launchPlanError={null}
    onSave={async () => { saveCalls++; return details; }} />;
}

function draw() {
  root.render(<React.StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}>
      <RestoredConfiguration />
    </main>
  </InstanceSettingsSaveProvider></I18nProvider></React.StrictMode>);
}

async function run() {
  await restoreInstanceArchive("retained-archive");
  check(restored, "The synthetic archive restore completed before configuration opens");
  const pendingList = listInstanceArchives();
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); await frame(); });
  await waitForConfigurationResult();
  check(inventoryPending, "Archive inventory stays in progress while the restored configuration mounts");
  check(Boolean(fixture.querySelector('[data-field-key="server_name"] input')),
    "The first configuration load succeeds while archive inventory is still busy");
  check(!fixture.querySelector('[data-configuration-state="error"]'), "The first open never shows an inventory-lock error");
  check(moduleReads.length > 0 && moduleReads.every((read) => read.counts === false && read.inventoryPending),
    "The selected-module hook requests current configuration without unrelated preserved-program counts");
  check(fixture.querySelector<HTMLInputElement>('[data-field-key="server_name"] input')?.value === "Retained world",
    "Opening a restored instance preserves its saved configuration");
  check(saveCalls === 0, "Opening configuration does not write or replace the restored settings");

  const readsBeforeLibrary = moduleReads.length;
  let libraryReadSettled = false;
  const pendingLibrary = readModuleDetails(moduleDetails.summary.id).then(
    () => { libraryReadSettled = true; return null; },
    (cause: unknown) => { libraryReadSettled = true; return cause; }
  );
  await act(async () => { await frame(); });
  check(inventoryPending && !libraryReadSettled && moduleReads.length === readsBeforeLibrary,
    "Ordinary library reads wait for archive inventory without skipping preserved-program counts");
  moduleFailure = "Fixture native inventory read failure";
  inventory.resolve({ archives: [], pending_deletions: [], issues: [] });
  await pendingList;
  check(String(await pendingLibrary).includes("Fixture native inventory read failure"),
    "Queued library reads propagate their real native failure instead of silently skipping it");
  check(moduleReads.at(-1)?.counts === true && !moduleReads.at(-1)?.inventoryPending,
    "Queued library reads request preserved-program counts only after inventory completes");
  moduleFailure = null;
  const libraryDetails = await readModuleDetails(moduleDetails.summary.id);
  check(libraryDetails.summary.install_state === "Installed" && libraryDetails.schema_json === moduleDetails.schema_json
    && libraryDetails.default_ports[0]?.port === 25565 && libraryDetails.process?.executable === "server.exe",
    "Inventory-free configuration and ordinary reads retain the descriptor's installation, schema, ports and process data");

  moduleFailure = "Fixture descriptor cannot be read";
  await act(async () => { requestRetry(); await frame(); });
  check(Boolean(fixture.querySelector('[data-configuration-state="error"]')), "A real configuration read failure remains visible locally");
  const afterFailure = moduleReads.length;
  await act(async () => { draw(); await frame(); });
  check(moduleReads.length === afterFailure, "A failed read does not silently retry on render");
  moduleFailure = null;
  await act(async () => {
    const retry = fixture.querySelector<HTMLButtonElement>('[data-configuration-state="error"] button.secondary-button');
    check(retry, "The local failure provides a retry action");
    retry.click(); await frame();
  });
  check(Boolean(fixture.querySelector('[data-field-key="server_name"] input'))
    && !fixture.querySelector('[data-configuration-state="error"]'), "Explicit retry reloads the same configuration without reloading the application");
  check(moduleReads.length === afterFailure + 1, "Explicit retry performs one fresh read instead of reusing a cached failure");
  check(saveCalls === 0, "Failure and recovery never save an unready configuration");
  await act(async () => { root.unmount(); });
  check(errors.length === 0, "React and the browser report no rendering errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
