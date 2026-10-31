import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { I18nProvider } from "../../src/i18n";
import type { InstanceDetails } from "../../src/types";
import { ConfigurationWorkspace, type ConfigurationWorkspaceProps } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import { InstanceSettingsDraftInvalidError } from "../../src/views/settings/instance-settings-save-queue";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100dvh;display:grid;grid-template-rows:44px minmax(0,1fr) 40px;padding:12px;gap:12px";
const root = createRoot(fixture);
const store = createActivityNoticeStore();
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  return { browser_errors: errors, native_dialogs: 0 };
} });
let checks = 0;
let coordinator: ReturnType<typeof useInstanceSettingsSaveCoordinator>;
function SaveObserver() {
  coordinator = useInstanceSettingsSaveCoordinator();
  return null;
}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  check(found, `Missing ${selector}`);
  return found;
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function until(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(async () => { await frame(); });
  }
}
async function pressEnter(target: HTMLElement) {
  await act(async () => {
    target.focus();
    check(document.activeElement === target, "Activity action must accept keyboard focus");
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native keyboard dispatch failed");
  });
}
async function fill(selector: string, value: string) {
  await act(async () => {
    const input = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const validationAction = (field: string) => `[data-configuration-validation-field="${field}"]`;
const fieldInput = (field: string) => `#configuration-unturned-${field.replaceAll("_", "-")}-input`;
const errorNotices = () => store.getSnapshot().filter((notice) => notice.tone === "error");

async function run() {
  await prepareBrowserLocaleCatalogs();
  const descriptor = await readModuleDetails("unturned");
  const settings = { ...buildMockSettingsForModule("unturned", "Unturned Dedicated Server", "validation-fixture"),
    internet_server: true, game_server_login_token: "" };
  const details: InstanceDetails = {
    summary: { id: "validation-fixture", module_id: "unturned", name: "Unturned Dedicated Server", status: "Stopped",
      active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
    settings_json: JSON.stringify(settings), ports: descriptor.default_ports,
    config_file_path: "C:/synthetic/validation/settings.json", saves_path: "C:/synthetic/validation/saves",
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5
  };
  const saved: string[] = [];
  const props: ConfigurationWorkspaceProps = {
    details, moduleDetails: descriptor, moduleDetailsError: null,
    onRetryModuleDetails: () => { throw new Error("Unexpected retry"); },
    bindAddressCandidates: [], runtime: null, launchPlan: null, launchPlanError: null,
    onSave: async (input) => {
      saved.push(input.settings_json);
      props.details = { ...props.details, settings_json: input.settings_json };
      render();
      return props.details;
    }
  };
  let mounted = true;
  let generation = 0;
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
      <SaveObserver />
      <header><strong>Unturned Dedicated Server</strong><div>配置</div></header>
      <ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: "关闭" }}>
        <div style={{ minHeight: 0, overflow: "hidden", border: "1px solid var(--shell-line)" }}>
          {mounted ? <ConfigurationWorkspace key={generation} {...props} /> : null}
        </div>
        <footer className="shell-activity-bar"><span className="shell-activity-label">动态</span>
          <ActivityBar store={store} entries={[]} />
        </footer>
      </ActivityNoticeTarget.Provider>
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  async function openDetails() {
    await pressEnter(element(".shell-activity-expand"));
    await until(() => Boolean(fixture.querySelector(".shell-activity-panel")), "Activity details did not open");
  }
  async function jump(field: string, expanded = false) {
    await pressEnter(element(`${expanded ? ".shell-activity-panel" : ".shell-activity-viewport"} ${validationAction(field)}`));
    await until(() => document.activeElement === fixture.querySelector(fieldInput(field)), `Focus did not reach ${field}`);
    check(!fixture.querySelector(".shell-activity-panel"), "Navigation left activity details over the target");
    const box = element(fieldInput(field)).getBoundingClientRect();
    const main = element(".configuration-workspace__main").getBoundingClientRect();
    check(box.top >= main.top - 1 && box.bottom <= main.bottom + 1, "Target field was not scrolled into view");
  }
  await act(async () => { render(); await document.fonts.ready; });
  await until(() => errorNotices().length > 0, "Initial validation was not published");
  check(errorNotices().length === 1 && errorNotices()[0].text.includes("GSLT"), "Related fields duplicated the GSLT error");
  check(!fixture.querySelector(".configuration-workspace__validation"), "Validation banner still occupies the editor");
  check(!element(".configuration-workspace__main").querySelector(".shell-activity-notice, .form-note--error"),
    "Visible validation text still occupies the editor");
  const internet = element<HTMLInputElement>(fieldInput("internet_server"));
  check(internet.getAttribute("aria-invalid") === "true", "Internet option lost its invalid state");
  check(internet.closest(".configuration-field")?.classList.contains("is-invalid"), "Internet option lost its red frame");
  const description = document.getElementById(internet.getAttribute("aria-errormessage") ?? "");
  check(description?.textContent?.includes("GSLT"), "The invalid option lost its accessible error message");
  check(getComputedStyle(description).position === "absolute" && description.getBoundingClientRect().height <= 1,
    "Accessible error still consumes a field row");
  const roomPosition = element(fieldInput("map")).getBoundingClientRect().top;
  check(saved.length === 0, "Invalid settings were saved");
  checks++;

  await jump("internet_server");
  checks++;
  await openDetails();
  check(element(".shell-activity-panel").querySelectorAll(".shell-activity-notice.is-error").length === 1,
    "Expanded details duplicated a shared error");
  check(element(".shell-activity-panel").querySelectorAll("[data-configuration-validation-field]").length === 2,
    "Grouping lost a related field action");
  await jump("game_server_login_token", true);
  checks++;

  await fill(fieldInput("game_server_login_token"), "0123456789ABCDEF0123456789ABCDEF");
  check(errorNotices().length === 0, "Correcting the token left stale validation notices");
  check(!element(fieldInput("game_server_login_token")).hasAttribute("aria-invalid"), "Corrected field is still invalid");
  await until(() => saved.length > 0, "Corrected settings did not save");
  check(JSON.parse(saved.at(-1)!).game_server_login_token === "0123456789ABCDEF0123456789ABCDEF", "Save lost the corrected token");
  checks++;

  await fill(fieldInput("game_server_login_token"), "");
  await until(() => errorNotices().length === 1, "A recurring validation error was not published");
  await jump("internet_server");
  check(Math.abs(element(fieldInput("map")).getBoundingClientRect().top - roomPosition) < 1,
    "Validation navigation moved room controls");
  await act(async () => { element<HTMLInputElement>(fieldInput("internet_server")).click(); });
  check(errorNotices().length === 0, "Disabling Internet mode did not clear both related errors");
  check(Math.abs(element(fieldInput("map")).getBoundingClientRect().top - roomPosition) < 1,
    "Removing validation shifted the room layout");
  checks++;

  await act(async () => { element<HTMLInputElement>(fieldInput("internet_server")).click(); });
  await until(() => errorNotices().length === 1, "Restoring Internet mode did not restore validation");
  await openDetails();
  await pressEnter(element(".shell-activity-panel .shell-activity-notice.is-error .shell-activity-notice-close"));
  check(errorNotices().length === 0, "Dismissed validation is still displayed");
  check(element(fieldInput("internet_server")).getAttribute("aria-invalid") === "true", "Dismissing a notice cleared the error state");
  const savedBeforeFlush = saved.length;
  await act(async () => {
    const failure = await coordinator.flush(details.summary.id).then(() => null, (error: unknown) => error);
    check(failure instanceof InstanceSettingsDraftInvalidError, "Dismissing validation allowed invalid settings to be flushed");
  });
  check(saved.length === savedBeforeFlush, "Dismissed validation persisted an invalid draft");
  await pressEnter(element(".shell-activity-panel-close"));
  await act(async () => { element<HTMLInputElement>(fieldInput("internet_server")).click(); });
  await act(async () => { element<HTMLInputElement>(fieldInput("internet_server")).click(); });
  await until(() => errorNotices().length === 1, "A dismissed error stayed hidden after resolution and recurrence");
  checks++;

  await fill(fieldInput("max_players"), "0");
  await until(() => errorNotices().length > 1, "A second validation problem was lost");
  await openDetails();
  check(element(".shell-activity-panel").querySelector(validationAction("max_players")), "Additional errors have no direct navigation");
  await jump("max_players", true);
  await fill(fieldInput("max_players"), "24");
  check(errorNotices().length === 1, "Correcting one field removed unrelated validation");
  checks++;

  await act(async () => { element<HTMLInputElement>(fieldInput("internet_server")).click(); });
  const navigation = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  await act(async () => {
    if (navigation.getClientRects().length && navigation.getAttribute("aria-expanded") !== "true") navigation.click();
  });
  await act(async () => { element<HTMLButtonElement>('[data-configuration-section-id="network"] > button').click(); });
  const port = element<HTMLInputElement>(".instance-port-fields__input");
  const portValue = port.value;
  const portBounds = port.getBoundingClientRect();
  await fill(".instance-port-fields__input", "0");
  check(port.getAttribute("aria-invalid") === "true", "Invalid network port lost its error marker");
  check(!fixture.querySelector(".instance-port-fields__error"), "Network validation still inserts an error row");
  check(Math.abs(port.getBoundingClientRect().top - portBounds.top) < 1, "Network validation shifted port controls");
  await openDetails();
  const portAction = [...element(".shell-activity-panel").querySelectorAll<HTMLButtonElement>("button[aria-controls]")]
    .find((button) => button.getAttribute("aria-controls") === port.id);
  check(portAction, "Network error lost its direct port action");
  await pressEnter(portAction);
  check(document.activeElement === port && !fixture.querySelector(".shell-activity-panel"), "Network error action did not focus its port");
  await act(async () => {
    const failure = await coordinator.flush(details.summary.id).then(() => null, (error: unknown) => error);
    check(failure instanceof InstanceSettingsDraftInvalidError, "Invalid network draft passed the save barrier");
  });
  await fill(".instance-port-fields__input", portValue);
  check(errorNotices().length === 0, "Correcting a network port retained its error notice");
  checks++;

  await act(async () => { mounted = false; render(); });
  check(store.getSnapshot().length === 0, "Leaving the workspace retained obsolete notices");
  checks++;
  // Leave the reproduced Unturned error visible for an optional screenshot.
  props.details = details;
  await act(async () => { mounted = true; generation++; render(); });
  await until(() => Boolean(fixture.querySelector(".shell-activity-viewport .shell-activity-notice.is-error")), "Final error did not render");
  const footer = element(".shell-activity-bar");
  check(footer.scrollWidth <= footer.clientWidth + 1, "Validation actions overflow the activity bar");
  for (const action of footer.querySelectorAll<HTMLElement>("[data-configuration-validation-field]")) {
    const box = action.getBoundingClientRect();
    check(box.width > 20 && box.left >= 0 && box.right <= innerWidth, "A direct navigation action is clipped");
  }
  await act(async () => {
    await Promise.all(footer.getAnimations({ subtree: true })
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity)
      .map((animation) => animation.finished));
  });
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, saves: saved.length, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
