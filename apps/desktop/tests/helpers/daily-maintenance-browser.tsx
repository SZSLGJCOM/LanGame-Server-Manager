import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n } from "../../src/i18n";
import { DailyMaintenanceEditor } from "../../src/views/servers/DailyMaintenanceEditor";
import type { DailyMaintenanceAction, DailyMaintenanceSchedule, UpdateDailyMaintenanceScheduleInput } from "../../src/maintenance-schedule";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
const reads: { instanceId: string; action: DailyMaintenanceAction }[] = [];
const writes: UpdateDailyMaintenanceScheduleInput[] = [];
const stored = new Map<string, DailyMaintenanceSchedule>();
const deferredReads: { instanceId: string; action: DailyMaintenanceAction; resolve: (value: DailyMaintenanceSchedule) => void }[] = [];
const deferredWrites: { input: UpdateDailyMaintenanceScheduleInput; resolve: (value: DailyMaintenanceSchedule) => void }[] = [];
let failNextWrite = false;
let deferWrites = false;

function policy(instanceId: string, action: DailyMaintenanceAction): DailyMaintenanceSchedule {
  return structuredClone(stored.get(`${instanceId}:${action}`) ?? {
    instance_id: instanceId, action, enabled: false, time_local: "04:00", revision: 0, last_run: null
  });
}
function persist(input: UpdateDailyMaintenanceScheduleInput): DailyMaintenanceSchedule {
  const current = policy(input.instanceId, input.action);
  if (current.revision !== input.expectedRevision) throw new Error("Policy revision conflict");
  const updated = { ...current, enabled: input.enabled, time_local: input.timeLocal, revision: current.revision + 1 };
  stored.set(`${input.instanceId}:${input.action}`, updated);
  return structuredClone(updated);
}
// Only the native IPC boundary is replaced. Production component state,
// translation, DOM events and read/update API envelopes remain unchanged.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => {
  if (command === "read_instance_maintenance_schedule") {
    if (Object.keys(args ?? {}).sort().join(",") !== "action,instanceId") throw new Error("Invalid read envelope");
    const query = args as { instanceId: string; action: DailyMaintenanceAction };
    reads.push(query);
    if (query.instanceId === "deferred-read") return new Promise<DailyMaintenanceSchedule>((resolve) => deferredReads.push({ ...query, resolve }));
    return Promise.resolve(policy(query.instanceId, query.action));
  }
  if (command === "update_instance_maintenance_schedule") {
    if (Object.keys(args ?? {}).join(",") !== "input") throw new Error("Invalid update envelope");
    const input = args!.input as UpdateDailyMaintenanceScheduleInput;
    if (Object.keys(input).sort().join(",") !== "action,enabled,expectedRevision,instanceId,timeLocal") throw new Error("Invalid policy input");
    writes.push(structuredClone(input));
    if (failNextWrite) { failNextWrite = false; return Promise.reject(new Error("Policy write rejected")); }
    if (deferWrites) return new Promise<DailyMaintenanceSchedule>((resolve) => deferredWrites.push({ input, resolve }));
    return Promise.resolve(persist(input));
  }
  throw new Error(`Unexpected native command: ${command}`);
} } });

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function input(action: DailyMaintenanceAction, field: "enabled" | "time"): HTMLInputElement {
  const node = fixture.querySelector<HTMLInputElement>(`input[name="daily-${action}-${field}"]`);
  if (!node) throw new Error(`Missing ${action} ${field}`);
  return node;
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function View({ instanceId, readOnly }: { instanceId: string; readOnly: boolean }) {
  const { t } = useI18n();
  return <main className="maintenance-workspace" style={{ padding: 24, maxWidth: 1000, margin: "0 auto" }}>
    <div className="maintenance-workspace__section settings-schema-section" style={{ display: "grid" }}>
      {(["backup", "restart"] as const).map((action) => <section key={action}
        className="server-workbench-surface server-maintenance-card" aria-label={t(`servers.dailyMaintenance.${action}`)}>
        <DailyMaintenanceEditor instanceId={instanceId} action={action} readOnly={readOnly} t={t} />
      </section>)}
    </div>
  </main>;
}
async function render(instanceId: string, readOnly = false) {
  await act(async () => root.render(<StrictMode><I18nProvider><View instanceId={instanceId} readOnly={readOnly} /></I18nProvider></StrictMode>));
}
async function loaded() {
  await settle(() => fixture.querySelector<HTMLInputElement>('input[name="daily-backup-enabled"]')?.disabled === false
    && fixture.querySelector<HTMLInputElement>('input[name="daily-restart-enabled"]')?.disabled === false, "Maintenance policies did not load");
}
async function toggle(action: DailyMaintenanceAction) {
  check(!input(action, "enabled").disabled, `${action} toggle must be available`);
  await act(async () => input(action, "enabled").click());
}
async function typeTime(action: DailyMaintenanceAction, value: string) {
  const node = input(action, "time");
  await act(async () => {
    node.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function blurTime(action: DailyMaintenanceAction) { await act(async () => input(action, "time").blur()); }

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await render("daily-a"); await loaded();
  check(!input("backup", "enabled").checked && !input("restart", "enabled").checked, "Both policies must default to disabled");
  check(input("backup", "time").value === "04:00" && input("restart", "time").value === "04:00", "Default local times must be visible");
  check(writes.length === 0 && reads.some((read) => read.action === "backup") && reads.some((read) => read.action === "restart"), "Loading must read both independent policies without writing");
  check(input("backup", "enabled").labels?.[0]?.textContent?.includes("每日定时备份"), "Backup checkbox needs its visible accessible label");
  check(input("restart", "time").labels?.[0]?.textContent?.includes("本机时间"), "Time control must identify its local-time basis");

  await toggle("backup"); await settle(() => writes.length === 1 && !input("backup", "enabled").disabled, "Checkbox save did not finish");
  check(writes[0].instanceId === "daily-a" && writes[0].action === "backup" && writes[0].enabled && writes[0].expectedRevision === 0, "Checkbox change must immediately submit the canonical input");
  check(policy("daily-a", "backup").revision === 1 && !input("restart", "enabled").checked, "Backup save must not change restart policy");
  await typeTime("backup", "05:45");
  check(writes.length === 1 && input("backup", "time").value === "05:45", "Time draft must remain local before blur");
  await blurTime("backup"); await settle(() => writes.length === 2 && !input("backup", "time").disabled, "Time blur save did not finish");
  check(writes[1].timeLocal === "05:45" && writes[1].expectedRevision === 1 && policy("daily-a", "backup").revision === 2, "Blur save must use the latest persisted revision");
  await toggle("restart"); await settle(() => writes.length === 3 && !input("restart", "enabled").disabled, "Restart toggle save did not finish");
  await typeTime("restart", "06:15"); await blurTime("restart");
  await settle(() => writes.length === 4 && !input("restart", "time").disabled, "Restart time save did not finish");
  check(writes[2].action === "restart" && writes[2].expectedRevision === 0 && writes[3].expectedRevision === 1, "Restart must own its independent revision sequence");
  check(policy("daily-a", "restart").time_local === "06:15" && policy("daily-a", "backup").time_local === "05:45", "Backup and restart times must remain independent");

  failNextWrite = true; await typeTime("backup", "07:10"); await blurTime("backup");
  await settle(() => fixture.textContent?.includes("Policy write rejected") === true, "Failed save must display its error");
  check(input("backup", "time").value === "07:10" && policy("daily-a", "backup").time_local === "05:45", "Failed save must retain the unsaved draft without changing persisted data");
  check(!input("backup", "time").disabled && writes.length === 5, "Failed save must settle without an automatic retry");
  const reload = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "重新加载策略");
  check(reload && !reload.disabled, "Failed policy must offer explicit reload");
  await act(async () => reload.click());
  await settle(() => input("backup", "time").value === "05:45" && !fixture.textContent?.includes("Policy write rejected"), "Explicit reload must replace the failed draft with persisted state");
  check(writes.length === 5, "Reload must read rather than repeat the failed write");

  await render("deferred-read");
  check(input("backup", "enabled").disabled && fixture.textContent?.includes("加载"), "Pending reads must show loading and disable edits");
  check(deferredReads.length === 2, "Both old-instance reads must be captured at the IPC boundary");
  await render("daily-b"); await loaded();
  await act(async () => { for (const pending of deferredReads) pending.resolve({ ...policy(pending.instanceId, pending.action), enabled: true, time_local: "23:59", revision: 99 }); });
  check(!input("backup", "enabled").checked && input("backup", "time").value === "04:00"
    && !input("restart", "enabled").checked && input("restart", "time").value === "04:00", "Late reads must never replace a different instance's controls");
  deferWrites = true; await toggle("backup");
  check(deferredWrites.length === 1 && input("backup", "enabled").disabled, "Pending save must own and disable its policy controls");
  const writesBeforeDuplicate = writes.length;
  await act(async () => input("backup", "enabled").click());
  check(writes.length === writesBeforeDuplicate, "Disabled pending save must not duplicate the write");
  await render("daily-c"); await loaded(); deferWrites = false;
  await act(async () => { for (const pending of deferredWrites) pending.resolve(persist(pending.input)); });
  check(!input("backup", "enabled").checked && input("backup", "time").value === "04:00", "Late save must never replace a different instance's controls");
  check(policy("daily-b", "backup").enabled && !policy("daily-c", "backup").enabled, "Late write must persist only to its original instance");

  const beforeArchive = { reads: reads.length, writes: writes.length };
  await render("daily-archive", true);
  check(input("backup", "enabled").disabled && input("restart", "enabled").disabled
    && input("backup", "time").disabled && input("restart", "time").disabled, "Read-only policies must disable all controls");
  await act(async () => { input("backup", "enabled").click(); input("restart", "enabled").click(); });
  check(reads.length === beforeArchive.reads && writes.length === beforeArchive.writes, "Read-only instances must perform no active policy reads or writes");
  check(fixture.textContent?.includes("归档恢复后保留策略时间"), "Read-only policy behavior must be explained");

  await render("daily-a"); await loaded();
  check(input("backup", "time").value === "05:45" && input("restart", "time").value === "06:15", "Returning to an instance must reread its saved independent policies");
  check(input("backup", "enabled").checked && input("restart", "enabled").checked, "Returning must preserve both persisted enabled states");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Daily policy controls must not overflow the viewport");
  for (const node of fixture.querySelectorAll<HTMLInputElement>("input")) {
    const bounds = node.getBoundingClientRect();
    check(bounds.width > 12 && bounds.height > 12 && bounds.left >= 0 && bounds.right <= innerWidth
      && bounds.top >= 0 && bounds.bottom <= innerHeight, `Control ${node.name} must be visible and usable`);
  }
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, reads: reads.length, writes: writes.length,
    instance_isolation: true, read_only_isolation: true };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Daily maintenance fixture stalled after ${checks} checks`)), 25000);
})]).finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
