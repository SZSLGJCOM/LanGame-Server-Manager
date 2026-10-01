import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, updateInstance } from "../../src/api";
import { I18nProvider } from "../../src/i18n";
import { SystemView } from "../../src/views/SystemView";
import type { InstanceDetails, PortBinding } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const noOperation = () => {};
const opened: string[] = [];
let checks = 0;
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, `${description}: ${fixture.querySelector(".system-instance-list")?.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const ports = (port: number): PortBinding[] => [{ name: "query", protocol: "udp", port: port + 1 },
  { name: "game", protocol: "udp", port }];
async function savePorts(details: InstanceDetails, nextPorts: PortBinding[]) {
  let updated!: InstanceDetails;
  // Canvas measurement and renderer setup may finish while storage is pending.
  await act(async () => {
    updated = await updateInstance({ id: details.summary.id, bind_ip: "0.0.0.0", ports: nextPorts,
      settings_json: details.settings_json, auto_backup_on_stop: details.auto_backup_on_stop,
      backup_retention_count: details.backup_retention_count }, details.settings_json);
  });
  return updated;
}
async function run() {
  const { locale } = await fetch("/__system_connection_locale").then((response) => response.json()) as { locale: "zh-CN" | "en-US" };
  localStorage.setItem("langame.locale", locale);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: true });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  const second = bootstrap.state.instances.find((instance) => instance.module_id === "arksurvivalascended");
  check(original && second, "Mock must provide two game instances");
  let selected = await savePorts(await readInstanceDetails(original.id), ports(28888));
  const other = await savePorts(await readInstanceDetails(second.id), ports(29888));
  const summaries = [selected.summary, other.summary].map((entry) => ({ ...entry, status: "Stopped", active_process_count: 0 }));
  localStorage.setItem(`langame.join-address.${selected.summary.id}`, "192.0.2.42");
  const props: ComponentProps<typeof SystemView> = {
    snapshot: bootstrap.state.snapshot, instances: summaries, appSettings: bootstrap.state.settings,
    bindAddressCandidates: [{ address: "0.0.0.0", kind: "all" }, { address: "192.0.2.42", kind: "lan" },
      { address: "198.51.100.4", kind: "overlay" }],
    steamCmdStatus: null, steamCmdBusy: false, steamCmdProgress: null, steamCmdMessage: "",
    onOpenInstance: (id) => { opened.push(id); }, onPickDirectory: async () => null, onSaveAppSettings: noOperation,
    onEnsureSteamCmd: noOperation, onUninstallSteamCmd: noOperation
  };
  const render = async () => { await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    root.render(<StrictMode><I18nProvider><SystemView {...props} /></I18nProvider></StrictMode>);
  }); };
  const row = (name: string) => [...fixture.querySelectorAll<HTMLButtonElement>(".system-instance-item")]
    .find((entry) => entry.querySelector(".instance-name")?.textContent === name)!;
  const address = (name: string) => row(name)?.querySelector<HTMLElement>(".system-instance-item-address");
  await render();
  await settleUntil(() => address(selected.summary.name)?.textContent?.includes("192.0.2.42:28888") === true
    && address(other.summary.name)?.textContent?.includes("198.51.100.4:29888") === true,
  "System overview must use saved game ports and preferred joinable addresses, not wildcard or port count");
  check(!fixture.querySelector(".system-instance-list")?.textContent?.includes("0.0.0.0"), "Bind wildcard must not masquerade as a player address");
  checks++;

  selected = await savePorts(selected, ports(28898));
  props.instances = summaries.map((entry) => ({ ...entry }));
  props.bindAddressCandidates = [{ address: "192.0.2.42", kind: "lan" }];
  await render();
  await settleUntil(() => address(selected.summary.name)?.textContent?.includes("192.0.2.42:28898") === true
    && address(other.summary.name)?.textContent?.includes("192.0.2.42:29888") === true,
  "A refreshed system overview must read updated saved ports and available LAN addresses");
  checks++;

  await act(async () => { row(selected.summary.name).click(); });
  await act(async () => { row(other.summary.name).focus(); });
  check(document.activeElement === row(other.summary.name), "Overview rows must remain keyboard focusable");
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter dispatch failed");
  });
  check(opened.length === 2 && opened[0] === selected.summary.id && opened[1] === other.summary.id,
    "Click and Enter must still open their own instance");
  checks++;

  selected = await savePorts(selected, []);
  props.instances = summaries.map((entry) => ({ ...entry }));
  await render();
  const unavailable = locale === "zh-CN" ? "暂无可用连接地址" : "No join address available";
  await settleUntil(() => address(selected.summary.name)?.textContent?.trim() === unavailable,
    "An empty saved port list must show an unavailable address in the active language");
  check(!address(selected.summary.name)?.textContent?.includes(":28898"), "Missing ports must not retain a stale endpoint");
  checks++;

  props.instances = [{ ...summaries[0], id: "system-fixture-missing-instance" }];
  await render();
  const failed = locale === "zh-CN" ? "连接信息读取失败" : "Connection unavailable";
  await settleUntil(() => address(selected.summary.name)?.textContent?.trim() === failed,
    "A rejected storage read must show a localized failure without inventing an address");
  check(!address(selected.summary.name)?.textContent?.includes("0.0.0.0"), "Read failure must not fabricate a bind endpoint");
  checks++;

  selected = await savePorts(selected, ports(28898));
  props.instances = summaries.map((entry) => ({ ...entry }));
  await render();
  await settleUntil(() => address(selected.summary.name)?.textContent?.includes("192.0.2.42:28898") === true,
    "A later successful read must recover connection text");
  const panel = fixture.querySelector<HTMLElement>(".system-instance-panel")!;
  await act(async () => { panel.scrollIntoView({ block: "center", inline: "nearest" }); });
  for (const entry of [row(selected.summary.name), row(other.summary.name)]) {
    const outer = entry.getBoundingClientRect();
    const label = entry.querySelector<HTMLElement>(".system-instance-item-address")!;
    const labelBox = label.getBoundingClientRect();
    check(outer.width > 0 && outer.left >= -1 && outer.right <= innerWidth + 1, "Instance row must fit its viewport");
    check(entry.scrollWidth <= entry.clientWidth + 1 && label.scrollWidth <= label.clientWidth + 1,
      "Instance row and saved connection text must not overflow or truncate");
    check(labelBox.left >= outer.left && labelBox.right <= outer.right && labelBox.height > 0,
      "Saved connection text must remain visible inside its row");
  }
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, locale, browser_errors: errors, opened_instances: opened,
    displayed_connections: [...fixture.querySelectorAll(".system-instance-item-address")].map((entry) => entry.textContent) };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`System connection interaction stalled after ${checks} checks`)), 20000);
})]).finally(() => {
  clearTimeout(watchdog);
  // The page remains mounted for capture after the test act scopes have ended.
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
})
  .catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
