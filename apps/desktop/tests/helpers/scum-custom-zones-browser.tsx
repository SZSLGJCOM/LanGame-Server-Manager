import { act } from "react";
import { createRoot } from "react-dom/client";
import { readInstanceDetails, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import { buildScumCustomZoneDamageInput, readScumCustomZones, writeScumCustomZoneDamage } from "../../src/scum-custom-zones";
import type { InstanceDetails, InstanceProvisioning, ModuleDetails } from "../../src/types";
import { ScumCustomZonesRenderer } from "../../src/views/settings/ScumCustomZonesRenderer";
import "../../src/app.css";
import "../../src/views/settings/scum-settings.css";
import { ConfigurationActivityHost } from "./configuration-activity-host";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const host = document.getElementById("fixture")!;
const root = createRoot(host);
const cases: string[] = [];
const errors: string[] = [];
const originalError = console.error;
console.error = (...args) => { errors.push(`stage=${cases.length} ` + args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
let details: InstanceDetails;
let descriptor: ModuleDetails;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const field = (key: string) => host.querySelector<HTMLSelectElement>(`[data-field-key="custom_zones.${key}"] select`)!;
const button = (text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((node) => node.textContent === text)!;
async function until(condition: () => boolean) {
  const deadline = performance.now() + 6000;
  while (!condition()) {
    if (performance.now() >= deadline) throw new Error(`UI condition timed out: ${host.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  }
}
async function render() {
  await act(async () => { root.render(<I18nProvider><ConfigurationActivityHost><main className="configuration-workspace" style={{ padding: 24 }}>
    <ScumCustomZonesRenderer sectionId="custom-zones" details={details} moduleDetails={descriptor}
      settings={{}} disabled={false} onPatch={() => { throw new Error("Zone changes cannot use ordinary settings saves"); }} />
  </main></ConfigurationActivityHost></I18nProvider>); });
}
async function change(key: string, value: string) {
  await act(async () => { const node = field(key); node.value = value; node.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function click(text: string) { await act(async () => { button(text).click(); }); }
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  check(!Object.hasOwn(window, "__TAURI_INTERNALS__"), "This fixture must use synthetic transport");
  descriptor = await readModuleDetails("scum");
  const created = await invokeMock<InstanceProvisioning>("create_instance_record", { input: { module_id: "scum", name: "Zone review" } });
  details = await readInstanceDetails(created.summary.id);
  await render(); await until(() => !!field("melee"));
  check(!field("melee").disabled && field("melee").value === "allow", "A real Stopped status must admit editing");
  check(host.querySelectorAll('[data-field-key^="custom_zones."] select').length === 3, "Three independent rules");
  check(host.textContent?.includes("玩家伤害规则保持不变"), "Explain affected actors");
  cases.push("stopped configuration and bilingual controls");
  const before = await readScumCustomZones(details.summary.id);
  await change("melee", "block");
  check((await readScumCustomZones(details.summary.id)).revision === before.revision, "Draft must not auto-save");
  await click("备份并保存区域规则"); await until(() => !!host.textContent?.includes("区域规则已保存"));
  const after = await readScumCustomZones(details.summary.id);
  check(after.configurations[0].damage.melee === "block" && !!after.backup_id, "Save must confirm values and backup");
  check(JSON.stringify(after.configurations[1]) === JSON.stringify(before.configurations[1]), "Other configuration preserved");
  cases.push("explicit save, backup receipt and preserved other zones");
  await change("projectile", "block");
  await writeScumCustomZoneDamage(buildScumCustomZoneDamageInput(after, after.configurations[1], { melee: "allow", melee_weapon: "block", projectile: "allow" }));
  await click("备份并保存区域规则"); await until(() => !!host.querySelector('[role="alert"]'));
  check(field("projectile").value === "block", "Rejected save must retain the draft");
  check(!host.querySelector(".scum-custom-zones [role=alert]") && host.querySelector(".shell-activity-bar [role=alert]"),
    "Rejected saves appear in activity without moving zone controls");
  await click("放弃修改并重新读取"); await until(() => !!field("projectile") && field("projectile").value === "allow");
  cases.push("stale revision rejection and explicit discard/reload");
  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  await render(); await until(() => !!host.textContent?.includes("请先停止服务器"));
  check(!button("备份并保存区域规则") || button("备份并保存区域规则").disabled, "Running instance cannot save");
  await act(async () => { details = await readInstanceDetails(created.summary.id); });
  await render(); await until(() => !!field("melee") && !field("melee").disabled);
  cases.push("running state lock and stopped-state recovery");
  await act(async () => { await document.fonts.ready; });
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Editor must fit viewport");
  check(errors.length === 0, "No browser or React errors");
  cases.push("layout and browser health");
  return { status: "passed", cases, browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); }); console.error = originalError;
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error: unknown) => ({ status: "failed", cases, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
