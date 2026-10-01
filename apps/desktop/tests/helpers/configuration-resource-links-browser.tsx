import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { ModuleDetails } from "../../src/types";
import { ConfigurationField } from "../../src/views/settings/ConfigurationField";
import { buildConfigurationFieldCopy } from "../../src/views/settings/GuidedSettingsForm";
import { parseGuidedSettingsSchema } from "../../src/views/settings/guided-settings";
import type { GuidedSettingsField } from "../../src/views/settings/settings-schema";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const requests: { url: string; target?: string; features?: string; failed: boolean }[] = [];
let catalogRenders = 0;
let patches = 0;
let failNextOpen = false;
const originalOpen = window.open;
window.open = (url, target, features) => {
  const failed = failNextOpen;
  failNextOpen = false;
  requests.push({ url: String(url), target, features, failed });
  if (failed) throw new Error("Synthetic browser launch failure");
  return null;
};
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

const cases = [
  { moduleId: "dontstarve", key: "cluster_token", game: "饥荒联机版",
    url: "https://accounts.klei.com/account/game/servers?game=DontStarveTogether" },
  { moduleId: "unturned", key: "game_server_login_token", game: "Unturned",
    url: "https://steamcommunity.com/dev/managegameservers" },
  { moduleId: "theforest", key: "steam_account_token", game: "The Forest",
    url: "https://steamcommunity.com/dev/managegameservers" },
  { moduleId: "projectzomboid", key: "discord_token", game: "Project Zomboid",
    url: "https://discord.com/developers/applications/select/bot" }
];
const syntheticSecret = "synthetic-browser-fixture-secret";
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = fixture.querySelector<T>(selector);
  check(target, `Missing ${selector}`);
  return target;
}
function fieldRoot(key: string) { return element(`[data-field-key="${key}"]`); }
function input(key: string) { return element<HTMLInputElement>(`[data-field-key="${key}"] input`); }
function link(key: string) { return element<HTMLAnchorElement>(`[data-field-key="${key}"] a.configuration-field-resource-link`); }
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, `${message}; ${errors.join("; ")}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function nativeKey(key: "Tab" | "Enter" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: "POST" });
    check(response.ok, `Native ${key} dispatch failed`);
  });
}
function fits() {
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Document must not overflow horizontally");
  for (const target of fixture.querySelectorAll<HTMLElement>("section, .configuration-field, input, .configuration-field-resource-link")) {
    const bounds = target.getBoundingClientRect();
    check(bounds.width > 0 && bounds.left >= -1 && bounds.right <= innerWidth + 1,
      `Configuration content escapes the viewport: ${target.className}`);
    check(target.scrollWidth <= target.clientWidth + 1, `Configuration content overflows: ${target.className}`);
  }
}
function Fields({ modules }: { modules: ModuleDetails[] }) {
  const { locale, t } = useI18n();
  useEffect(() => { catalogRenders++; }, [t]);
  const schemas = modules.map((module) => parseGuidedSettingsSchema(module, locale, t));
  function renderField(field: GuidedSettingsField | undefined, moduleId: string) {
    check(field, `Real schema field is missing for ${moduleId}`);
    return <ConfigurationField field={field} copy={buildConfigurationFieldCopy(t)} t={t}
      idPrefix={`resource-${moduleId}`} settings={{ [field.key]: syntheticSecret }} value={syntheticSecret}
      onPatch={() => { patches++; }} />;
  }
  return <main className="configuration-workspace" style={{ padding: 24, height: "100vh", display: "block", overflow: "auto" }}>
    <h2 id="fixture-heading" tabIndex={-1}>服务器配置</h2>
    <button id="before-fields" className="secondary-button" style={{ marginBottom: 20 }}>配置前的操作</button>
    <div className="configuration-workspace__content settings-schema-section"
      style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 32, maxWidth: 1120 }}>
      {cases.map((entry, index) => <section key={entry.key} className="configuration-workspace__main" style={{ display: "block", minWidth: 0 }}>
        <h3>{entry.game}</h3>
        <div className="settings-schema-grid" style={{ gridTemplateColumns: "minmax(0, 1fr)" }}>
          {renderField(schemas[index].fields.find((field) => field.key === entry.key), entry.moduleId)}
          {index === 0 ? renderField(schemas[index].fields.find((field) => field.key === "cluster_password"), entry.moduleId) : null}
        </div>
      </section>)}
    </div>
  </main>;
}
function verifyOpen(index: number, expectedUrl: string) {
  check(requests.length === index + 1, "Activating a title must open exactly one page");
  const request = requests[index];
  check(request.url === expectedUrl && !request.url.includes(syntheticSecret), "Only the fixed official URL may be opened");
  check(request.target === "_blank" && request.features?.includes("noopener") && request.features.includes("noreferrer"),
    "Opening the official page must retain the external navigation protections");
  check(patches === 0, "Opening a resource must not change configuration");
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  check(!document.querySelector(".configuration-field-help-tooltip"), "Unmount must remove field help portals");
  window.open = originalOpen;
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  const [modules] = await Promise.all([
    Promise.all(cases.map((entry) => readModuleDetails(entry.moduleId))),
    import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")
  ]);
  await act(async () => { root.render(<I18nProvider><Fields modules={modules} /></I18nProvider>); });
  await settleUntil(() => fixture.querySelectorAll(".configuration-field-resource-link").length === 4 && catalogRenders >= 2,
    "The four real resource fields did not mount");
  await act(async () => { await document.fonts.ready; await new Promise(requestAnimationFrame); });
  fits();
  await act(async () => { element("#before-fields").focus(); });
  await nativeKey("Tab");
  check(document.activeElement === link(cases[0].key), "Tab must reach the first resource title");

  for (const entry of cases) {
    const anchor = link(entry.key);
    const control = input(entry.key);
    check(anchor.href === entry.url && anchor.tabIndex === 0, `${entry.key} must expose a keyboard-accessible official link`);
    const labelIds = control.getAttribute("aria-labelledby")?.split(/\s+/) ?? [];
    check(labelIds.includes(anchor.id) && anchor.id.length > 0 && Boolean(anchor.textContent?.trim()),
      `${entry.key} must retain its accessible field name`);
    check(control.type === "password" && control.value === syntheticSecret, `${entry.key} must remain concealed`);
    let requestIndex = requests.length;
    await act(async () => { anchor.click(); });
    verifyOpen(requestIndex, entry.url);
    requestIndex = requests.length;
    await act(async () => { anchor.focus(); });
    await nativeKey("Enter");
    verifyOpen(requestIndex, entry.url);
    check(control.type === "password" && control.value === syntheticSecret, "Resource navigation must preserve the secret value and visibility");
  }

  const ordinary = input("cluster_password");
  check(ordinary.type === "password" && !fieldRoot("cluster_password").querySelector("a"), "Ordinary passwords must not gain a resource link");
  check(ordinary.labels?.length === 1 && ordinary.labels[0].htmlFor === ordinary.id,
    "Ordinary password titles must remain associated labels");

  let requestIndex = requests.length;
  failNextOpen = true;
  await act(async () => { link(cases[0].key).click(); });
  verifyOpen(requestIndex, cases[0].url);
  await settleUntil(() => Boolean(fixture.querySelector('[role="alert"]')), "Failed external opening must be visible");
  check(element('[role="alert"]').textContent?.includes("Synthetic browser launch failure"), "The error notice must retain the failure reason");
  fits();
  requestIndex = requests.length;
  await act(async () => { link(cases[0].key).click(); });
  verifyOpen(requestIndex, cases[0].url);
  await settleUntil(() => !fixture.querySelector('[role="alert"]'), "Successful retry must clear the error notice");
  await act(async () => { element("#fixture-heading").focus(); });
  await nativeKey("Escape");
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Final screenshot must show the normal state");
  fits();
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", resource_fields: cases.length, open_requests: requests.length,
    successful_opens: requests.filter((request) => !request.failed).length, patches, browser_errors: errors,
    pointer_events: "DOM click", keyboard_events: "native CDP Tab and Enter", failure_recovered: true };
}
void run().catch((error) => ({ status: "failed", error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
