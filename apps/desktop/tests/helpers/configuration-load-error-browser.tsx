import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readModuleDetails } from "../../src/api";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { LocaleCode } from "../../src/i18n";
import { ConfigurationWorkspace, type ConfigurationWorkspaceProps } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100dvh;display:grid;grid-template-rows:52px minmax(0,1fr) 44px;padding:16px;gap:12px";
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let locale = "zh-CN";
let changeLocale: (value: LocaleCode) => void;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = fixture.querySelector<T>(selector);
  check(target, `Missing ${selector}`);
  return target;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function pressEnter(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  await act(async () => {
    target.focus();
    check(document.activeElement === target, "Error recovery control must accept keyboard focus");
    check(getComputedStyle(target).outlineStyle !== "none", "Keyboard focus must be visible");
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native keyboard dispatch failed");
  });
}
function fits(target: HTMLElement) {
  const box = target.getBoundingClientRect();
  check(box.width > 0 && box.height > 0 && box.left >= 0 && box.right <= innerWidth + 1,
    `Control is outside the viewport: ${target.className}`);
  check(target.scrollWidth <= target.clientWidth + 1, `Horizontal overflow: ${target.className}`);
}
function LanguageObserver() {
  const current = useI18n();
  changeLocale = current.setLocale;
  locale = current.locale;
  return null;
}

async function run() {
  // Only storage/IPC boundaries use synthetic development data. The workspace,
  // save coordinator, localization, notice portal and application CSS are real.
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(original, "Development fixture must contain an instance");
  const stored = await readInstanceDetails(original.id);
  const sourceModule = await readModuleDetails("minecraft");
  const details = { ...stored, summary: { ...stored.summary, id: "configuration-recovery-fixture",
    module_id: "configuration-fixture", name: "Configuration recovery fixture" }, settings_json: "{}", ports: [] };
  const moduleDetails = { ...sourceModule, summary: { ...sourceModule.summary, id: "configuration-fixture" },
    schema_json: JSON.stringify({ type: "object", properties: { fixture_name: { type: "string", title: "Server name" } } }) };
  let retryCount = 0;
  const saved: string[] = [];
  const props: ConfigurationWorkspaceProps = {
    details, moduleDetails: null, moduleDetailsError: null, bindAddressCandidates: [],
    runtime: null, launchPlan: null, launchPlanError: null,
    onRetryModuleDetails: () => {
      retryCount++;
      props.moduleDetails = null;
      props.moduleDetailsError = null;
      render();
    },
    onSave: async (input) => { saved.push(input.settings_json); return { ...props.details, settings_json: input.settings_json }; }
  };
  const noticeTarget = document.createElement("div");
  noticeTarget.className = "shell-activity-notices";
  let generation = 0;
  function render() {
    root.render(<StrictMode><I18nProvider><LanguageObserver /><InstanceSettingsSaveProvider>
      <header><strong>LanGame Server Manager</strong><div>Configuration recovery · synthetic fixture</div></header>
      <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
        <main style={{ minHeight: 0, overflow: "hidden", border: "1px solid var(--shell-line)", borderRadius: 12 }}>
          <ConfigurationWorkspace key={generation} {...props} />
        </main>
      </ActivityNoticeTarget.Provider>
      <footer className="shell-activity-bar" ref={(host) => { if (host) host.appendChild(noticeTarget); }} />
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector('[data-configuration-state="loading"]')), "Loading state did not mount");
  await document.fonts.ready;
  check(retryCount === 0 && saved.length === 0, "Loading must not retry or save by itself");
  checks++;
  const rawError = "Unable to read module metadata: synthetic archive inventory conflict.\n" + "long-diagnostic-segment/".repeat(45);
  async function fail() {
    await act(async () => { props.moduleDetails = null; props.moduleDetailsError = rawError; render(); });
    await settleUntil(() => Boolean(fixture.querySelector('[data-configuration-state="error"]')), "Error state did not mount");
  }
  const themes: Record<string, string> = {};
  for (const language of ["zh-CN", "en-US"] as const) {
    await act(async () => { changeLocale(language); });
    await settleUntil(() => locale === language, "Locale did not update");
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      generation++;
      await fail();
      const state = element(".configuration-load-error");
      const disclosure = element<HTMLDetailsElement>(".configuration-load-error__details");
      check(!disclosure.open, "Technical diagnostics must initially be collapsed");
      const expected = language === "zh-CN" ? "重新加载配置" : "Reload configuration";
      check(element(".configuration-load-error__retry").textContent === expected, "Recovery action must be localized");
      check(element(".shell-activity-notice").parentElement === noticeTarget, "Failure feedback belongs in the activity bar");
      check(!element(".shell-activity-notice").textContent?.includes(rawError), "The activity bar must not expose a wall of technical text");
      check(!state.querySelector("textarea,input"), "Failed metadata must not expose an editable configuration");
      fits(state);
      fits(element(".configuration-load-error__retry"));
      await pressEnter(element(".configuration-load-error__details > summary"));
      check(disclosure.open, "Keyboard Enter must expand diagnostics");
      const diagnostic = element(".configuration-load-error__details > pre");
      check(diagnostic.textContent === rawError, "Technical diagnostics must preserve the full original error");
      fits(diagnostic);
      const style = getComputedStyle(diagnostic);
      check(style.fontSize === "13px" && style.whiteSpace === "pre-wrap", "Diagnostics must use the shared readable code role");
      themes[theme] = style.backgroundColor;
      check(getComputedStyle(element(".configuration-load-error__heading > h2")).fontSize === "16px", "Error title must use the shared section role");
      await pressEnter(element(".shell-activity-notice-close"));
      check(!fixture.querySelector(".shell-activity-notice") && state.isConnected,
        "Dismissing feedback must preserve the unavailable configuration and retry action");
      checks++;
    }
  }
  check(themes.dark !== themes.light, "Diagnostics must follow theme changes");
  check(retryCount === 0 && saved.length === 0, "Viewing or dismissing an error must not retry or save");
  await pressEnter(element(".configuration-load-error__retry"));
  check(retryCount === 1 && !!fixture.querySelector('[data-configuration-state="loading"]'),
    "Explicit retry must transition to loading exactly once");
  check(!fixture.querySelector(".configuration-workspace"), "Retry must not imply a successful load");
  await fail();
  check(element(".configuration-load-error__details > pre").textContent === rawError,
    "A failed retry must remain visible with its diagnostic");
  await pressEnter(element(".configuration-load-error__retry"));
  await act(async () => { props.moduleDetails = moduleDetails; props.moduleDetailsError = null; render(); });
  await settleUntil(() => Boolean(fixture.querySelector(".configuration-workspace")), "Successful retry did not restore the editor");
  check(retryCount === 2 && !fixture.querySelector(".configuration-load-error"), "Only success may remove the error state");
  checks++;

  generation++;
  await act(async () => { props.details = { ...details, settings_json: "{ invalid JSON" }; render(); });
  const repair = element<HTMLTextAreaElement>(".configuration-load-error__repair > textarea");
  check(repair.value === "{ invalid JSON" && !fixture.querySelector(".configuration-load-error__retry"),
    "Malformed settings must retain their draft and expose repair rather than a metadata retry");
  check(repair.labels?.length === 1, "JSON repair must retain an accessible label");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(repair, '{"fixture_name":"Repaired"}');
    repair.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await settleUntil(() => Boolean(fixture.querySelector(".configuration-workspace")), "Repairing JSON did not restore the editor");
  checks++;
  generation++;
  props.details = details;
  await act(async () => { props.moduleDetails = { ...moduleDetails, schema_json: "{ broken schema" }; render(); });
  check(!!fixture.querySelector(".configuration-load-error") && !fixture.querySelector(".configuration-load-error__repair"),
    "Invalid schema must remain an explicit unavailable state without changing instance data");
  checks++;
  // Leave a representative localized request failure for visual inspection.
  const finalLocale = innerWidth >= 1200 ? "zh-CN" : "en-US";
  document.documentElement.dataset.theme = innerWidth >= 1200 ? "dark" : "light";
  await act(async () => { changeLocale(finalLocale); });
  await settleUntil(() => locale === finalLocale, "Final locale did not load");
  generation++;
  await fail();
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, retry_count: retryCount, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", checks,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
