import { act } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import { InstanceConnectionSettingsPanel } from "../../src/views/settings/InstanceConnectionSettingsPanel";
import type { CreateInstanceInput, InstanceDetails, ModuleDetails, PortBinding, SteamCmdStatus } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const checks: string[] = [];
const errors: string[] = [];
const creations: CreateInstanceInput[] = [];
let nativeDialogs = 0;
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { nativeDialogs++; throw new Error("Unexpected native dialog"); };

const inventory: ModuleProgramInventory = { installations: [], creation: {
  can_create: true, action: "existing_install", program_path: "fixture/program", additional_bytes: 0, reason: null
} };
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string) => {
  if (command === "inspect_module_programs") return inventory;
  throw new Error(`Unexpected native command: ${command}`);
} } });
const steamCmd: SteamCmdStatus = { ready: true, executable_exists: true, root: "fixture/steamcmd",
  executable_path: "fixture/steamcmd/steamcmd.exe", configured_root: "fixture/steamcmd",
  configured_executable_path: "fixture/steamcmd/steamcmd.exe", source: "configured", ownership: "managed", can_uninstall: true };
const ports: PortBinding[] = [{ name: "game", port: 8777, protocol: "udp" }, { name: "console", port: 1234, protocol: "tcp" }];
function descriptor(moduleId: string, installed = true): ModuleDetails {
  return { summary: { id: moduleId, name: moduleId === "astroneer" ? "ASTRONEER" : "Valheim", version: "fixture",
    install_state: installed ? "Installed" : "NotInstalled", supported_platforms: ["windows"], steam_app_id: 728470 },
    schema_json: "{}", default_ports: ports, runtime: { port_roles: [
      { role: "player", port_names: ["game"] }, { role: "service", port_names: ["console"] }
    ] } };
}
function details(moduleId: string, publicIp: string): InstanceDetails {
  return { summary: { id: `${moduleId}-network-fixture`, name: "Network fixture", module_id: moduleId, status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "192.168.1.2" }, settings_json: JSON.stringify({ public_ip: publicIp }),
    ports, config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
    auto_backup_on_stop: false, backup_retention_count: 3, active_run: null };
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return result;
}
const bubbles = () => [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip")];
async function pause(milliseconds: number) {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, milliseconds)); });
}
async function until(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 1600;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(description);
    await pause(10);
  }
}
async function mouse(x: number, y: number) {
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", body: JSON.stringify({ type: "move", x, y }) });
    if (!response.ok) throw new Error("Native mouse dispatch failed");
  });
}
async function hover(target: HTMLElement) {
  const bounds = target.getBoundingClientRect();
  await mouse(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
}
async function key(value: "Tab" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" });
    if (!response.ok) throw new Error("Native key dispatch failed");
  });
}
function helpText(trigger: HTMLElement) {
  const description = document.getElementById(trigger.getAttribute("aria-describedby") ?? "");
  if (!description?.textContent) throw new Error("Network help must have an accessible description");
  return description.textContent;
}
async function opened(trigger: HTMLElement) {
  const text = helpText(trigger);
  await until(() => bubbles().length === 1 && bubbles()[0].textContent === text
    && getComputedStyle(bubbles()[0]).opacity === "1", "Expected one visible network help bubble");
  const bubble = bubbles()[0];
  const bounds = bubble.getBoundingClientRect();
  check(bounds.left >= 7 && bounds.right <= innerWidth - 7 && bounds.top >= 7 && bounds.bottom <= innerHeight - 7,
    "Network help stays inside the viewport");
  check(bubble.scrollWidth <= bubble.clientWidth + 1, "Network help wraps without horizontal scrolling");
  return bubble;
}
async function closeHelp() {
  await key("Escape");
  await mouse(innerWidth - 4, innerHeight - 4);
  await until(() => bubbles().length === 0, "Network help must close on Escape");
}
async function draw(locale: string, moduleId: string, publicIp = "", installed = true) {
  await mouse(innerWidth - 4, innerHeight - 4);
  localStorage.setItem("langame.locale", locale);
  const module = descriptor(moduleId, installed);
  const instance = details(moduleId, publicIp);
  await act(async () => { root.render(<I18nProvider key={`${locale}-${moduleId}`}>
    <main style={{ height: "calc(100vh - 48px)", overflowY: "auto", margin: 24 }}>
      <div style={{ display: "grid", gridTemplateColumns: "320px minmax(0, 1fr)", gap: 32, alignItems: "start" }}>
        <section className="library-detail-page" style={{ display: "block", width: 320, padding: 0 }}>
          <LibraryServerActions selected={module.summary} selectedModuleDetails={module} steamCmdStatus={steamCmd}
            steamCmdBusy={false} installLabel="Installed" installBusy={false} installStatusClass="is-success"
            creating={false} instanceName="Network fixture" onInstanceNameChange={() => {}}
            onInstall={() => {}} onUninstall={() => {}} onCreateServer={async (input) => { creations.push(input); }} />
        </section>
        <section className="network-fixture" style={{ minWidth: 0 }}>
          <InstanceConnectionSettingsPanel details={instance} moduleDetails={module} bindAddressCandidates={[]}
            bindIp={instance.summary.bind_ip} ports={ports} defaultPorts={ports} onPortsChange={() => {}} onBindIpChange={() => {}} />
        </section>
      </div>
    </main>
  </I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector<HTMLButtonElement>('button[type="submit"]')
    || fixture.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled) {
    if (performance.now() > deadline) throw new Error("Creation did not become ready");
    await act(frame);
  }
}
function checkLayout(locale: string) {
  const sidebar = element(".library-detail-page").getBoundingClientRect();
  const trigger = element(".library-sidebar-server-actions .astroneer-network-requirements");
  check(trigger.getBoundingClientRect().right <= sidebar.right + 1 && trigger.scrollWidth <= trigger.clientWidth + 1,
    `${locale}: the small help entry fits the 320px creation sidebar`);
  for (const help of fixture.querySelectorAll<HTMLElement>(".astroneer-network-requirements")) {
    const row = help.closest<HTMLElement>(".astroneer-network-requirements-row")!;
    const label = row.querySelector<HTMLElement>(".detail-label")!;
    const bounds = help.getBoundingClientRect();
    check(bounds.height <= 24 && row.getBoundingClientRect().height <= 24,
      `${locale}: networking help occupies only the existing label row`);
    check(bounds.left >= label.getBoundingClientRect().right && !help.textContent?.includes("IPv4"),
      `${locale}: the compact trigger does not contain the explanatory paragraphs`);
  }
  const network = element(".network-fixture");
  check(network.scrollWidth <= network.clientWidth + 1, `${locale}: network settings stay inside their column`);
  const submit = element<HTMLButtonElement>('button[type="submit"]');
  submit.scrollIntoView({ block: "nearest" });
  const scroll = element("main").getBoundingClientRect();
  const bounds = submit.getBoundingClientRect();
  check(bounds.top >= scroll.top && bounds.bottom <= scroll.bottom, `${locale}: creation remains reachable by local scrolling`);
  check(document.documentElement.scrollWidth <= innerWidth, `${locale}: document has no horizontal overflow`);
}
async function run() {
  await prepareBrowserLocaleCatalogs();
  for (const locale of ["en-US", "zh-CN"]) {
    await draw(locale, "astroneer", "", false);
    check(bubbles().length === 0, `${locale}: explanatory paragraphs are closed by default`);
    const sidebarHelp = element(".library-sidebar-server-actions .astroneer-network-requirements");
    const sidebarText = helpText(sidebarHelp);
    check(sidebarHelp.compareDocumentPosition(element('button[type="submit"]')) & Node.DOCUMENT_POSITION_FOLLOWING,
      `${locale}: help is available before installation or instance creation`);
    check(sidebarText.includes(locale === "zh-CN" ? "官方在线服务" : "official online services"),
      `${locale}: the bubble explains the native server's online service requirement`);
    check(sidebarText.includes(locale === "zh-CN" ? "同一 Radmin 网络" : "same Radmin network")
      && sidebarText.includes(locale === "zh-CN" ? "公布地址" : "advertised address")
      && sidebarText.includes(locale === "zh-CN" ? "可达入口" : "reachable entry point"),
      `${locale}: Radmin requires a shared network and matching advertised address; public connections require a reachable entry point`);
    check(!/不支持断网纯局域网|无法.*局域网|必须.*独享公网|Offline LAN hosting is not supported/.test(sidebarText),
      `${locale}: networking help does not rule out all LAN solutions`);
    const networkHelp = element(".instance-connection-settings .astroneer-network-requirements");
    check(helpText(networkHelp).includes(locale === "zh-CN" ? "玩家入站连通性未验证" : "Inbound player connectivity has not been verified"),
      `${locale}: the network bubble does not claim inbound reachability`);
    check(helpText(networkHelp).includes(locale === "zh-CN" ? "出口 IPv4" : "outbound IPv4"),
      `${locale}: the bubble explains outbound address discovery`);
    checkLayout(locale);

    await act(async () => { element<HTMLButtonElement>(".library-install-state-button").focus(); });
    for (let step = 0; step < 4 && document.activeElement !== sidebarHelp; step++) {
      await key("Tab");
      await pause(20);
    }
    check(document.activeElement === sidebarHelp, `${locale}: native Tab reaches the networking help entry`);
    await opened(sidebarHelp);
    check(getComputedStyle(sidebarHelp).outlineStyle !== "none", `${locale}: keyboard focus has a visible indicator`);
    await closeHelp();
    check(document.activeElement === sidebarHelp, `${locale}: Escape closes help without losing keyboard focus`);

    await hover(networkHelp);
    const networkBubble = await opened(networkHelp);
    await hover(networkBubble);
    await pause(300);
    check(bubbles().length === 1 && bubbles()[0] === networkBubble, `${locale}: the hovered bubble stays open while being read`);
    await closeHelp();
    await act(async () => { element<HTMLButtonElement>('button[type="submit"]').click(); });
    check(creations.at(-1)?.module_id === "astroneer", `${locale}: private local address does not falsely block instance creation`);
    await draw(locale, "astroneer", "1.1.1.1");
    const configuredHelp = element(".instance-connection-settings .astroneer-network-requirements");
    await hover(configuredHelp);
    const configuredBubble = await opened(configuredHelp);
    check(configuredBubble.textContent?.includes(locale === "zh-CN" ? "玩家入站连通性未验证" : "Inbound player connectivity has not been verified"),
      `${locale}: a populated public IPv4 does not manufacture a successful connectivity result`);
    await closeHelp();
    await draw(locale, "valheim");
    check(!fixture.querySelector(".astroneer-network-requirements"), `${locale}: other games retain their own networking behavior`);
  }
  await draw("zh-CN", "astroneer");
  checkLayout("zh-CN-final");
  if (innerWidth <= 1000) {
    const trigger = element(".instance-connection-settings .astroneer-network-requirements");
    await hover(trigger);
    await opened(trigger);
  }
  check(errors.length === 0, "No React or browser errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, creations, browser_errors: errors };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); await frame(); });
  check(bubbles().length === 0, "Unmount removes the network help portal");
  return { browser_errors: errors, native_dialogs: nativeDialogs };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
