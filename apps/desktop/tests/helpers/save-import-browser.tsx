import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import type { SaveImportResult } from "../../src/save-import";
import { SaveImportPanel } from "../../src/views/servers/SaveImportPanel";
import { MaintenanceWorkspace } from "../../src/views/servers/MaintenanceWorkspace";
import { SaveImportOperationsProvider } from "../../src/views/servers/SaveImportOperations";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let source = "D:/old-server/world";
let failCheck = false;
let supported = true;
let failSupport = false;
let sourceHint = "world/level.dat + world/region";
let reads = 0;
let imports = 0;
let previews = 0;
let resolveImport: ((value: SaveImportResult) => void) | undefined;
let changed = 0;
let active = true;
let details: InstanceDetails;

function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); checks++; }
function select<T extends Element>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}`);
  return result;
}
async function click(selector: string) { await act(async () => select<HTMLButtonElement>(selector).click()); }
async function wait(predicate: () => boolean) {
  const expires = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() > expires) throw new Error("Save import UI did not settle");
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  }
}
async function openImport() {
  await wait(() => Boolean(fixture.querySelector(".save-import > .maintenance-chip__summary")));
  const toggle = select<HTMLButtonElement>(".save-import > .maintenance-chip__summary");
  if (toggle.getAttribute("aria-expanded") !== "true") await act(async () => toggle.click());
}
async function navigate(id: string) {
  const toggle = select<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") await act(async () => toggle.click());
  await click(`[data-configuration-section-id="${id}"] > button`);
}
async function input(value: string) {
  await act(async () => {
    const element = select<HTMLInputElement>('.save-import__source-row input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function render() {
  root.render(<StrictMode><I18nProvider><SaveImportOperationsProvider><main style={{ height: "calc(100vh - 48px)", padding: 24 }}>
    <MaintenanceWorkspace active={active} sections={[
      { id: "backups", title: "存档与备份", icon: "history", content: <p>备份列表</p> },
      { id: "save-import", title: "导入存档", icon: "folder", preserveAfterVisit: true,
        content: <SaveImportPanel key={details.summary.id} details={details} readOnly={false}
          onPickDirectory={async () => source} onChanged={() => { changed++; }} /> }
    ]} broadcast={null} />
  </main></SaveImportOperationsProvider></I18nProvider></StrictMode>);
}

async function run() {
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(instance => instance.module_id === "minecraft");
  check(instance, "Minecraft fixture is missing");
  details = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });
  details = { ...details, active_run: null, summary: { ...details.summary, status: "Stopped", active_process_count: 0 } };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, string>) => {
    if (command === "read_instance_save_import_support") {
      reads++;
      if (failSupport) throw new Error("Cannot inspect native saves");
      return { module_id: details.summary.module_id, supported, source_hint: sourceHint };
    }
    if (command === "preview_instance_save_import") {
      previews++;
      if (failCheck) throw new Error("Selected directory has no level.dat");
      return { instance_id: args.instanceId, module_id: details.summary.module_id, source_path: args.sourcePath,
        target_path: "D:/LanGame/instances/server-a/world", file_count: 4, total_bytes: 5120, existing_file_count: 2,
        existing_total_bytes: 1024, fingerprint: "checked-world", warnings: [],
        changes: [{ path: ".", file_count: 4, total_bytes: 5120 }] };
    }
    if (command === "import_instance_save") {
      check(args.expectedFingerprint === "checked-world", "Import did not use the inspected source identity");
      imports++;
      return new Promise<SaveImportResult>(resolve => { resolveImport = resolve; });
    }
    throw new Error(`Unexpected command: ${command}`);
  } } });
  await act(async () => render());
  await wait(() => Boolean(fixture.querySelector('[data-configuration-section-id="save-import"]')));
  check(reads === 0, "Unvisited import must not read storage");
  await navigate("save-import");
  check(reads === 0 && !fixture.querySelector('.save-import__source-row'), "A closed import popover must not read storage or render its form");
  await openImport();
  await wait(() => Boolean(fixture.querySelector('.save-import__source-row input')));
  check(!fixture.querySelector('.save-import details, .save-import__format'), "Import guidance must not be an inline disclosure");
  check(select('.save-import .maintenance-chip__heading').textContent?.includes("单机存档"), "Private world limitation is missing from the import help bubble");
  check(select('.save-import__source-field [role="tooltip"]').textContent?.includes("world/level.dat"), "Native requirements are missing from the source help bubble");
  check(![...select('.save-import__operation').querySelectorAll<HTMLElement>("p")].some(p => p.checkVisibility()), "Import popover shows explanatory prose");
  check(select<HTMLButtonElement>('.save-import__check').disabled, "Empty source can be checked");
  await click('.save-import__pick');
  check(select<HTMLInputElement>('.save-import__source-row input').value === source, "Directory picker lost the source");
  failCheck = true;
  await click('.save-import__check');
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("no level.dat"), "Validation failure lost its cause");
  check(imports === 0, "Validation attempted to import");
  failCheck = false;
  await click('.save-import__check');
  check(select('.save-import__preview').textContent?.includes("D:/LanGame/instances/server-a/world"), "Target is not reviewable");
  check(select('.save-import__changes').textContent?.includes("此存档目录"), "The inspected replacement scope is missing");
  check(select('.save-import__confirm').textContent?.includes("保留来源与服务器配置"), "Replacement confirmation lost the source and configuration boundary");
  check(select<HTMLButtonElement>('.save-import__submit').disabled, "Replacement needs acknowledgement");
  await click('.save-import__confirm input');
  await input("D:/another-world");
  check(!fixture.querySelector('.save-import__preview'), "Editing the source retained an old preview");
  await click('.save-import__check');
  check(!select<HTMLInputElement>('.save-import__confirm input').checked, "A new preview retained replacement acknowledgement");
  const readsBeforeSettingsChange = reads;
  sourceHint = "changed-world/level.dat + changed-world/region";
  details = { ...details, settings_json: details.settings_json + " " };
  await act(async () => render());
  check(!fixture.querySelector('.save-import__preview'), "Changed server configuration retained an old preview");
  await wait(() => Boolean(fixture.querySelector('.save-import__source-row input')));
  check(reads > readsBeforeSettingsChange && fixture.querySelector('.save-import__source-field [role="tooltip"]')?.textContent?.includes("changed-world/level.dat"),
    "Changed server configuration retained an old native source hint");
  await click('.save-import__check');
  await click('.save-import__confirm input');
  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  await act(async () => render());
  check(select<HTMLButtonElement>('.save-import__submit').disabled, "A running server can import");
  await click('.save-import__submit');
  check(imports === 0, "Running import reached IPC");
  details = { ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 } };
  await act(async () => render());
  await click('.save-import__submit');
  await click('.save-import__submit');
  check(imports === 1 && Boolean(resolveImport), "Pending import submitted more than once");
  check(select<HTMLInputElement>('.save-import__source-row input').disabled, "Source is editable during replacement");
  const originalDetails = details;
  details = { ...details, summary: { ...details.summary, id: "during-import" } };
  await act(async () => render());
  await openImport();
  await wait(() => Boolean(fixture.querySelector('.save-import__source-row input')));
  check(!fixture.querySelector('.save-import__result'), "Another instance inherited a pending import");
  await navigate("backups");
  active = false;
  await act(async () => render());
  await act(async () => resolveImport!({ instance_id: originalDetails.summary.id, saves_path: "D:/LanGame/instances/server-a/world",
    safeguard_backup_path: "D:/LanGame/instances/server-a/backups/pre-import",
    safeguard_backup_paths: ["D:/LanGame/instances/server-a/backups/pre-import", "D:/LanGame/instances/server-a/backups/pre-import-world-data"],
    copied_file_count: 4, copied_total_bytes: 5120, warnings: ["Recovery cleanup needs attention: D:/remaining-recovery"] }));
  active = true;
  details = originalDetails;
  await act(async () => render());
  await navigate("save-import");
  await openImport();
  check(fixture.textContent?.includes("等待启动验证"), "File copy was confused with successful game loading");
  check(select('.save-import__result').textContent?.includes("backups/pre-import"), "Recovery backup is missing");
  check(fixture.querySelectorAll('.save-import__backup-paths code').length === 2, "The result omitted a scope's safeguard backup");
  check(select('.save-import__result').textContent?.includes("恢复以上全部备份"), "Rollback instructions must cover every import scope");
  check(select('.save-import__result').textContent?.includes("remaining-recovery"), "A cleanup warning was hidden with format guidance");
  check(changed === 1, "Import did not refresh the owning instance");
  check(!fixture.querySelector('.save-import__preview'), "A completed import can be resubmitted");
  const retained = select('.save-import__result');
  await navigate("backups"); await navigate("save-import");
  check(select('.save-import__result') === retained, "Navigation lost the import result");
  check(previews === 4, "Unexpected source inspections were submitted");
  failSupport = true;
  details = { ...details, summary: { ...details.summary, id: "second-server" } };
  await act(async () => render());
  await openImport();
  await wait(() => Boolean(fixture.querySelector('[role="alert"]')));
  check(!fixture.querySelector('.save-import__result'), "A new instance inherited the previous import result");
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("Cannot inspect"), "Capability errors are hidden");
  failSupport = false; supported = false;
  await click('.save-import__operation > button');
  await wait(() => !fixture.querySelector('.save-import__operation > button'));
  check(fixture.textContent?.includes("当前游戏不支持"), "Unavailable file import must keep its limitation visible");
  check(!fixture.querySelector('.save-import__source-row'), "Unavailable importer offers an active file picker");
  details = { ...details, summary: { ...details.summary, id: "squad-server", module_id: "squad" } };
  await act(async () => render());
  await openImport();
  await wait(() => Boolean(fixture.textContent?.includes("没有可导入的持久世界存档")));
  check(!fixture.querySelector('.save-import__source-row'), "Squad offers a persistent-world import action");
  supported = true;
  details = originalDetails;
  await act(async () => render());
  await openImport();
  await wait(() => Boolean(fixture.querySelector('.save-import__source-row input')));
  await input(source);
  await act(async () => select<HTMLInputElement>('.save-import__source-row input')
    .dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
  await wait(() => Boolean(fixture.querySelector('.save-import__preview')));
  const importBox = select<HTMLButtonElement>('.save-import__submit').getBoundingClientRect();
  const scrollBox = select<HTMLElement>('.configuration-workspace__main').getBoundingClientRect();
  check(importBox.bottom <= Math.min(innerHeight, scrollBox.bottom) - 12, "The compact preview clips its import action outside the maintenance workspace");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Import layout overflows the window");
  const panel = select<HTMLElement>('.save-import');
  for (const element of panel.querySelectorAll<HTMLElement>('input, button, a, code')) {
    const box = element.getBoundingClientRect();
    check(box.width === 0 || box.left >= 0 && box.right <= innerWidth + 1, "An import control is clipped horizontally");
  }
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Import UI stalled")), 30000); })])
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
