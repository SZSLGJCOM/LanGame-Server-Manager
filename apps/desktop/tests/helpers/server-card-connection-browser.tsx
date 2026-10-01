import React, { act, StrictMode, useEffect, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, updateInstance } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import type { UiMessage } from "../../src/app-ui";
import type { InstanceConnectionInfo, InstanceDetails } from "../../src/types";
import { useInstanceConnections } from "../../src/hooks/useInstanceConnections";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let nativeDialogs = 0;
const rejectNativeDialog = () => { nativeDialogs++; throw new Error("Native browser dialog called"); };
window.confirm = rejectNativeDialog;
window.prompt = rejectNativeDialog;
window.alert = rejectNativeDialog;
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const selections: string[] = [];
const starts: string[] = [];
const notices: UiMessage[] = [];
const clipboardWrites: string[] = [];
let writeClipboard: (text: string) => Promise<void> = async () => {};
Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => {
  clipboardWrites.push(text);
  await writeClipboard(text);
} } });
let checks = 0;
const noOperation = () => {};

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(condition: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(element: HTMLElement | null) {
  check(element, "Expected a mounted control");
  await act(async () => { element.click(); });
}
async function key(name: "Tab" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
function contained(element: Element, container: Element, label: string) {
  const box = element.getBoundingClientRect();
  const outer = container.getBoundingClientRect();
  check(box.width > 0 && box.height > 0, `${label} is invisible`);
  check(box.left >= outer.left - 1 && box.right <= outer.right + 1 && box.top >= outer.top - 1 && box.bottom <= outer.bottom + 1,
    `${label} is clipped: ${JSON.stringify({ box: box.toJSON(), outer: outer.toJSON() })}`);
}
async function saveNetwork(details: InstanceDetails, port: number, bindIp = "0.0.0.0") {
  return updateInstance({ id: details.summary.id, bind_ip: bindIp, settings_json: details.settings_json,
    auto_backup_on_stop: details.auto_backup_on_stop, backup_retention_count: details.backup_retention_count,
    ports: [{ name: "game", protocol: "udp", port }, { name: "query", protocol: "udp", port: port + 1 }] }, details.settings_json);
}
async function checkConnectionRefresh(selected: InstanceDetails, other: InstanceDetails) {
  const host = document.createElement("div");
  host.hidden = true;
  document.body.appendChild(host);
  const probeRoot = createRoot(host);
  const published: string[] = [];
  const requests: { ids: string[]; resolve: (rows: InstanceConnectionInfo[]) => void }[] = [];
  let activeRequests = 0;
  let maximumConcurrentRequests = 0;
  let snapshot: ReturnType<typeof useInstanceConnections> | null = null;
  const toConnection = (details: InstanceDetails): InstanceConnectionInfo => ({ instance_id: details.summary.id,
    bind_ip: details.summary.bind_ip, ports: details.ports, settings_json: details.settings_json });
  const initialRows = [toConnection(selected), toConnection(other)];
  let input = { instances: [selected.summary, other.summary], selectedDetails: selected };
  const readConnections = (ids: string[]): Promise<InstanceConnectionInfo[]> => new Promise((resolve) => {
    activeRequests++;
    maximumConcurrentRequests = Math.max(maximumConcurrentRequests, activeRequests);
    let settled = false;
    requests.push({ ids: [...ids], resolve: (rows) => {
      check(!settled, "An IPC fixture request must settle only once");
      settled = true;
      activeRequests--;
      resolve(rows);
    } });
  });
  function Probe() {
    const state = useInstanceConnections(input.instances, input.selectedDetails, readConnections);
    snapshot = state;
    useEffect(() => { published.push(JSON.stringify(state.connections)); }, [state.connections]);
    return <output>{JSON.stringify(state.connections)}</output>;
  }
  const renderProbe = async () => { await act(async () => { probeRoot.render(<Probe />); }); };
  const rerenderPoll = async () => { input = structuredClone(input); await renderProbe(); };
  const resolveRequest = async (index: number, rows: InstanceConnectionInfo[]) => {
    check(requests[index], `Expected pending IPC request ${index}`);
    await act(async () => { requests[index].resolve(rows); });
  };
  try {
    await renderProbe();
    check(requests.length === 1, "The connection loader must start one initial request");
    for (let poll = 0; poll < 3; poll++) await rerenderPoll();
    check(requests.length === 1 && activeRequests === 1, "Equivalent poll objects must coalesce behind the slow request");
    await resolveRequest(0, initialRows);
    check(snapshot?.connections[selected.summary.id]?.ports[0].port === selected.ports[0].port,
      "Equivalent poll objects must not starve a completed connection response");
    check(requests.length === 2 && activeRequests === 1, "Coalesced polls must schedule one follow-up refresh");
    await resolveRequest(1, initialRows);
    check(activeRequests === 0 && maximumConcurrentRequests === 1, "Connection requests must remain single-flight");
    checks++;

    await rerenderPoll();
    const publicationsBeforeChange = published.length;
    const changedSelected = { ...selected, ports: [{ name: "game", protocol: "udp", port: 40000 }] };
    const changedOther = { ...other, summary: { ...other.summary, id: `${other.summary.id}-replacement` } };
    input = { instances: [changedSelected.summary, changedOther.summary], selectedDetails: changedSelected };
    await renderProbe();
    await resolveRequest(2, initialRows);
    check(published.length === publicationsBeforeChange, "A response from before an instance or port change must not publish");
    check(requests.length === 4 && requests[3].ids.join(",") === input.instances.map((entry) => entry.id).join(","),
      "Refresh after a configuration change must read the current instance identities");
    check(snapshot, "Probe must expose its current connection state");
    await act(async () => { snapshot!.acceptConnection(toConnection(changedSelected)); });
    const publicationsAfterCopy = published.length;
    const staleRows = [{ ...toConnection(changedSelected), ports: [{ name: "game", protocol: "udp", port: 39999 }] },
      toConnection(changedOther)];
    await resolveRequest(3, staleRows);
    check(published.length === publicationsAfterCopy && snapshot.connections[selected.summary.id]?.ports[0].port === 40000,
      "An older batch must not overwrite a fresh connection accepted by Copy");
    check(requests.length === 5 && maximumConcurrentRequests === 1, "Invalidated batches must preserve single-flight refresh");
    const currentRows = [toConnection(changedSelected), toConnection(changedOther)];
    await resolveRequest(4, currentRows);
    check(snapshot.connections[changedOther.summary.id]?.instance_id === changedOther.summary.id
      && !snapshot.connections[other.summary.id], "The next completed snapshot must replace removed instance connections");
    checks++;

    await rerenderPoll();
    check(requests.length === 6 && activeRequests === 1, "Unmount coverage needs an in-flight read");
    const publicationsBeforeUnmount = published.length;
    await act(async () => { probeRoot.unmount(); });
    await resolveRequest(5, currentRows);
    check(published.length === publicationsBeforeUnmount && requests.length === 6 && activeRequests === 0,
      "Unmounted connection owner must neither publish nor start a follow-up request");
    checks++;
  } finally {
    await act(async () => { probeRoot.unmount(); });
    host.remove();
  }
}
async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const minecraft = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  const ark = bootstrap.state.instances.find((instance) => instance.module_id === "arksurvivalascended");
  check(minecraft && ark, "Browser mock must provide the two game instances");
  let selected = await saveNetwork(await readInstanceDetails(minecraft.id), 28888);
  let other = await saveNetwork(await readInstanceDetails(ark.id), 29888);
  const stopped = (details: InstanceDetails, name: string): InstanceDetails => ({ ...details,
    summary: { ...details.summary, name, status: "Stopped", active_process_count: 0 }, active_run: null });
  selected = stopped(selected, "建造世界");
  other = stopped(other, "孤岛合作世界");
  localStorage.setItem(`langame.join-address.${selected.summary.id}`, "192.0.2.42");
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false,
    bindAddressCandidates: [{ address: "0.0.0.0", kind: "all" }, { address: "192.0.2.42", kind: "lan" },
      { address: "198.51.100.4", kind: "overlay", family_name: "联机网络" }],
    instances: [selected.summary, other.summary],
    moduleInstallations: { minecraft: { installState: "Installed", hasManagedInstallSource: true },
      arksurvivalascended: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: selected.summary.id, selectedDetails: selected, selectedBackups: [], selectedModuleDetails: null,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: (notice) => { notices.push(notice); }, onResumeAutoRefresh: noOperation,
    onSelectInstance: (id) => { selections.push(id); }, onStart: (id) => { starts.push(id); }, onStop: noOperation,
    onInstallModule: async () => {}, onOpenModuleLibrary: noOperation, onCreateInstance: noOperation,
    onPickDirectory: async () => null, onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: noOperation, onRestoreBackup: noOperation,
    onRenameBackup: async () => true, onDeleteBackup: noOperation, onSaveSettings: async () => undefined,
    onSaveAutostart: async () => {}, onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const render = async () => { await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); }); };
  await render();
  await settleUntil(() => fixture.querySelectorAll(".server-list-card-copy-action").length === 2, "Both cards must expose copy actions");
  const cards = [...fixture.querySelectorAll<HTMLElement>(".server-list-card")];
  const card = cards.find((entry) => entry.textContent?.includes("建造世界"))!;
  const otherCard = cards.find((entry) => entry.textContent?.includes("孤岛合作世界"))!;
  check(card && otherCard, "Expected both real server cards");
  const shell = card.querySelector<HTMLElement>(".server-list-card-shell")!;
  const copy = () => card.querySelector<HTMLButtonElement>(".server-list-card-copy-action")!;
  const otherCopy = () => otherCard.querySelector<HTMLButtonElement>(".server-list-card-copy-action")!;
  const start = () => [...card.querySelectorAll<HTMLButtonElement>(".server-list-card-footer button")]
    .find((button) => button.textContent?.trim() === "启动")!;
  const meta = () => card.querySelector<HTMLElement>(".server-list-card-meta")!;
  await settleUntil(() => meta().textContent?.includes("192.0.2.42:28888") === true
    && otherCard.querySelector(".server-list-card-meta")?.textContent?.includes("198.51.100.4:29888") === true,
  "Selected and unselected cards must display their saved game ports with joinable addresses");
  check(!meta().textContent?.includes("0.0.0.0") && !meta().textContent?.includes("端口 2"), "Card must not show bind wildcard or count as connection details");
  checks++;

  check(Math.abs(shell.getBoundingClientRect().height - 192) < 1 && Math.abs(card.getBoundingClientRect().height - 194) < 1,
    "The existing 192px card shell and 1px borders must remain");
  check(Math.abs(start().getBoundingClientRect().width - 88) < 1, "The existing 88px start width must remain");
  contained(copy(), card, "Copy action");
  contained(start(), card, "Start action");
  contained(meta(), card, "Connection label");
  check(copy().getBoundingClientRect().right <= start().getBoundingClientRect().left, "Copy action must sit left of Start");
  check(Boolean(copy().getAttribute("aria-label")?.includes("复制")), "Copy action needs an accessible name");
  checks++;

  let releaseClipboard: (() => void) | undefined;
  writeClipboard = () => new Promise<void>((resolve) => { releaseClipboard = resolve; });
  await click(copy());
  await settleUntil(() => clipboardWrites.length === 1, "First copy never reached clipboard");
  check(copy().disabled, "Copy must remain disabled while clipboard write is pending");
  await click(copy());
  check(clipboardWrites.length === 1 && notices.length === 0, "Pending copy must reject duplicate clicks and delay success feedback");
  check(clipboardWrites[0] === "192.0.2.42:28888", "Copy must use this card's selected join address and saved port");
  check(releaseClipboard, "Pending clipboard request must be owned by fixture");
  await act(async () => { releaseClipboard!(); });
  await settleUntil(() => !copy().disabled && copy().title === "已复制", "Successful copy must settle and report feedback");
  check(card.querySelector('[role="status"]')?.textContent === "已复制" && notices.length === 0, "Successful copy needs accessible feedback without errors");
  check(selections.length === 0 && starts.length === 0, "Copy must not select or start the instance");
  checks++;

  writeClipboard = async () => {};
  await click(otherCopy());
  await settleUntil(() => clipboardWrites.length === 2 && !otherCopy().disabled, "Unselected instance copy must complete");
  check(clipboardWrites[1] === "198.51.100.4:29888", "Unselected card must copy its own saved port");
  check(selections.length === 0 && starts.length === 0, "Unselected card copy must preserve selection and runtime");
  checks++;

  const fresh = await saveNetwork(selected, 28898);
  check(meta().textContent?.includes(":28888"), "Fixture must keep stale view props before fresh copy");
  await click(copy());
  await settleUntil(() => clipboardWrites.length === 3 && !copy().disabled, "Fresh-port copy did not settle");
  check(clipboardWrites[2] === "192.0.2.42:28898", "Copy must reread saved state instead of using stale card props");
  selected = stopped(fresh, "建造世界");
  props.selectedDetails = selected;
  props.instances = [selected.summary, other.summary];
  await render();
  await settleUntil(() => meta().textContent?.includes("192.0.2.42:28898") === true, "Saved network refresh must update card text");
  checks++;

  const noticeCount = notices.length;
  writeClipboard = async () => { throw new DOMException("Clipboard denied", "NotAllowedError"); };
  await click(copy());
  await settleUntil(() => !copy().disabled && notices.length === noticeCount + 1, "Clipboard denial must settle and report failure");
  check(notices[notices.length - 1]?.tone === "error" && notices[notices.length - 1]?.key === "activity.copyInviteFailed", "Clipboard denial must report an error, not success");
  check(copy().title === "复制失败，请重试" && card.querySelector('[role="status"]')?.textContent === "复制失败，请重试", "Clipboard denial needs visible and accessible feedback");
  check(selections.length === 0 && starts.length === 0, "Failed copy must not select or start the instance");
  checks++;

  writeClipboard = async () => {};
  await act(async () => { copy().focus(); });
  check(document.activeElement === copy(), "Copy must accept keyboard focus");
  await key("Tab");
  check(document.activeElement === start(), "Tab from Copy must reach Start");
  await act(async () => { copy().focus(); });
  await key("Enter");
  await settleUntil(() => clipboardWrites.length === 5 && !copy().disabled, "Enter must trigger one copy");
  check(clipboardWrites[4] === "192.0.2.42:28898", "Keyboard copy must use latest saved endpoint");
  check(selections.length === 0 && starts.length === 0, "Keyboard copy must not trigger card selection or Start");
  checks++;

  props.bindAddressCandidates = [];
  await render();
  await settleUntil(() => copy().disabled && otherCopy().disabled, "Wildcard without a usable interface must disable copy");
  check(!meta().textContent?.includes("0.0.0.0:"), "Unavailable connection must not look like a usable endpoint");
  const writesBeforeDisabledClick = clipboardWrites.length;
  await click(copy());
  check(clipboardWrites.length === writesBeforeDisabledClick, "Disabled connection must never write clipboard");
  checks++;

  props.bindAddressCandidates = [{ address: "192.0.2.42", kind: "lan" }];
  await render();
  await settleUntil(() => !copy().disabled && meta().textContent?.includes(":28898") === true, "Restored interface must restore copy");
  contained(copy(), card, "Final copy action");
  for (const [entry, expectedPort] of [[card, "28898"], [otherCard, "29888"]] as const) {
    const label = entry.querySelector<HTMLElement>(".server-list-card-meta")!;
    const address = label.querySelector<HTMLElement>(".server-list-card-address")!;
    const port = label.lastElementChild as HTMLElement;
    check(port?.textContent === expectedPort && getComputedStyle(port).display !== "none", "The current saved port must be rendered visibly");
    contained(port, label, "Actual port text");
    check(label.scrollWidth <= label.clientWidth + 1, "The visible connection text must not be clipped or ellipsized");
    check(getComputedStyle(address).display === (entry.clientWidth <= 250 ? "none" : "inline"),
      "Narrow cards must reserve room for the port; wider cards must retain the full address");
    check(label.title.includes(`192.0.2.42:${expectedPort}`), "The full connection address must remain available in its tooltip");
  }
  check(Math.abs(shell.getBoundingClientRect().height - 192) < 1 && Math.abs(card.getBoundingClientRect().height - 194) < 1 && Math.abs(start().getBoundingClientRect().width - 88) < 1,
    "Copy states must preserve the original card and Start dimensions");
  check(nativeDialogs === 0 && errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  await checkConnectionRefresh(selected, other);
  check(errors.length === 0, `Connection loader produced browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, native_dialogs: nativeDialogs, browser_errors: errors,
    card_width: Math.round(card.getBoundingClientRect().width), card_height: Math.round(card.getBoundingClientRect().height), card_shell_height: Math.round(shell.getBoundingClientRect().height), start_width: Math.round(start().getBoundingClientRect().width),
    copied_endpoints: clipboardWrites, selection_events: selections.length, start_events: starts.length };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} checks`)), 20_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
