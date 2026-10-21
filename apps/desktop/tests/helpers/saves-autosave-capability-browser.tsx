import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readModuleDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { nativeSavePolicyKeys } from "../../src/views/settings/save-policy";
import { ServerMaintenanceWorkspace } from "../../src/views/servers/ServerMaintenanceWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/instance-themes.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
// The full maintenance workspace loads slots, backups and schedules asynchronously; React's act() advisories
// about those background reads are not failures of this fixture.
console.error = (...args) => {
  const message = args.map(String).join(" ");
  if (!message.includes("not wrapped in act(")) errors.push(message);
  originalError(...args);
};
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
let details: InstanceDetails;
let moduleDetails: ModuleDetails | null = null;
let checks = 0;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function select<T extends Element>(selector: string): T {
  const value = fixture.querySelector<T>(selector); if (!value) throw new Error(`Missing ${selector}`); return value;
}
async function waitFor(ready: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!ready()) {
    if (performance.now() >= deadline) throw new Error(label);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function View() {
  const { locale, t } = useI18n();
  return <ServerMaintenanceWorkspace active details={details} moduleDetails={moduleDetails} locale={locale} t={t} />;
}
async function render(source: InstanceDetails, moduleId: string) {
  details = { ...source, summary: { ...source.summary, module_id: moduleId } };
  moduleDetails = await readModuleDetails(moduleId).catch(() => null);
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><View /></InstanceSettingsSaveProvider></I18nProvider>));
  await waitFor(() => Boolean(fixture.querySelector(".server-save-policy-card > .maintenance-chip__summary")), `${moduleId} saves header did not mount`);
}
const summary = () => select<HTMLButtonElement>(".server-save-policy-card > .maintenance-chip__summary");

async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((item) => item.module_id === "dontstarve");
  check(instance, "Isolated catalog fixture missing");
  const source = await readInstanceDetails(instance.id);

  // Capability comes from the native save-policy table, not a game list in the view.
  for (const moduleId of ["dontstarve", "satisfactory"]) {
    check(nativeSavePolicyKeys(moduleId).length > 0, `${moduleId} must declare native save settings`);
    await render(source, moduleId);
    check(!summary().hasAttribute("aria-disabled") && summary().getAttribute("aria-expanded") === "false",
      `${moduleId} keeps an openable autosave popover`);
    await act(async () => summary().click());
    check(summary().getAttribute("aria-expanded") === "true" && !select(".server-save-policy-card > .maintenance-chip__panel").hidden,
      `${moduleId} autosave popover must open`);
    await act(async () => summary().click());
  }

  check(nativeSavePolicyKeys("sevendaystodie").length === 0, "7 Days to Die declares no native save setting");
  await render(source, "sevendaystodie");
  const chip = summary();
  check(chip.getAttribute("aria-disabled") === "true" && !chip.disabled && chip.tabIndex === 0,
    "Unsupported autosave chip is aria-disabled yet focusable for its explanation");
  check(!fixture.querySelector(".server-save-policy-card .maintenance-chip__panel") && !chip.hasAttribute("aria-expanded"),
    "Unsupported autosave chip has no popover");
  await act(async () => chip.click());
  check(!fixture.querySelector(".server-save-policy-card .maintenance-chip__panel, .server-save-policy-card .server-save-policy-editor, .server-native-save-policies"),
    "Clicking the unsupported chip opens nothing");
  const description = document.getElementById(chip.getAttribute("aria-describedby") ?? "");
  check(description?.textContent?.includes("存档由游戏负责") && description.textContent.includes("保存频率"),
    "The chip describes why autosave is unavailable");
  await act(async () => chip.focus());
  await waitFor(() => Boolean(document.querySelector(".configuration-field-help-tooltip.is-visible")), "Focus did not reveal the reason bubble");
  const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")!;
  check(bubble.textContent?.includes("存档由游戏负责"), "The reason bubble shows the explanation");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", checks, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
