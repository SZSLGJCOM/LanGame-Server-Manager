import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { invokeMock } from "../../src/api-mock";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { buildMockPorts } from "../../src/api-mock/module-settings";
import { updateMockArkPorts } from "../../src/api-mock/ark-maps";
import { sendInstanceRuntimeCommand } from "../../src/api";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeOverview, InstanceRuntimeCommandResult, PortBinding } from "../../src/types";
import { ArkClusterMapsEditor } from "../../src/views/settings/ArkClusterMapsEditor";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { InstanceConnectionSettingsPanel } from "../../src/views/settings/InstanceConnectionSettingsPanel";
import { useInstancePortRegistration } from "../../src/views/settings/useInstancePortRegistration";
import { formatInstancePortName } from "../../src/views/settings/instance-port-presentation";
import { readAdditionalArkMaps, type AdditionalArkMap } from "../../src/views/settings/ark-cluster-maps";
import type { SettingsObject } from "../../src/views/settings/settings-schema";
import { bridge } from "./runtime-browser-events";
import type { RuntimeLogStreamEvent } from "../../src/runtime-log-stream";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
let checks = 0;
let editions = 0;
let patchCount = 0;
const commands: Array<Record<string, unknown>> = [];
const reads: number[] = [];
const sourceReads: Array<{ runId: number; source: unknown }> = [];
let documentRevision = 0;
function emitGameDocument(payload: RuntimeLogStreamEvent) {
  bridge.emit({ ...payload, snapshot_revision: ++documentRevision });
}
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); checks++; }
function element<T extends Element>(selector: string): T { const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node; }
async function change(field: HTMLInputElement | HTMLSelectElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(field instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(field, value);
    field.dispatchEvent(new Event(field instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function click(node: HTMLElement) { await act(async () => node.click()); }
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const addButton = () => element<HTMLButtonElement>(".ark-map-editor__add button");
const additionalRows = () => [...fixture.querySelectorAll<HTMLTableRowElement>(".ark-map-editor tbody tr")].slice(1);

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap");
  const sampleId = bootstrap.state.instances[0].id;
  const sample = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: sampleId });
  const runtimeSample = await invokeMock<InstanceRuntimeOverview>("read_instance_runtime_overview_from_storage", { instanceId: sampleId });
  let details: InstanceDetails = { ...sample, summary: { ...sample.summary, id: "ark-fixture", module_id: "arksurvivalascended", name: "群岛集群", status: "Stopped", active_process_count: 0 },
    ports: buildMockPorts("arksurvivalascended"), settings_json: "{}", active_run: null };
  let settings: SettingsObject = {};
  let mode: "maps" | "console" | "both" | "ports" = "maps";
  let terminal: ComponentProps<typeof RuntimeSurfaceWorkbench>;
  let visiblePorts: PortBinding[] = [];
  let portValidationBlocked = false;
  function PortProbe() {
    const moduleDetails = React.useMemo(() => buildMockModuleDetails(details.summary.module_id), [details.summary.module_id]);
    const registration = useInstancePortRegistration(details, moduleDetails);
    visiblePorts = registration.ports;
    return <InstanceConnectionSettingsPanel details={details} moduleDetails={moduleDetails} bindAddressCandidates={[]} bindIp={details.summary.bind_ip}
      ports={registration.ports} defaultPorts={registration.defaultPorts} onPortsChange={registration.setPorts} onBindIpChange={() => {}}
      onValidationBlockedChange={React.useCallback((blocked: boolean) => { portValidationBlocked = blocked; }, [])} />;
  }
  const maps = () => readAdditionalArkMaps(settings)!;
  function paint() {
    root.render(<StrictMode><I18nProvider>
      <div className={mode === "console" ? "server-detail-scroll--runtime" : undefined}
        style={{ display: "grid", gridTemplateColumns: mode === "both" ? "minmax(0, 1.15fr) minmax(0, 1fr)" : "minmax(0, 1fr)",
          gap: 24, minHeight: mode === "console" ? 0 : 650, height: mode === "console" ? "100%" : undefined,
          gridTemplateRows: mode === "console" ? "minmax(0, 1fr)" : undefined }}>
        {mode === "ports" ? <PortProbe key={details.summary.module_id} /> : null}
        {mode === "maps" || mode === "both" ? <ArkClusterMapsEditor key={details.summary.module_id} sectionId="transfer" details={details}
          moduleDetails={buildMockModuleDetails(details.summary.module_id)} settings={settings} disabled={false} onPatch={(patch) => {
            patchCount++; settings = { ...settings, ...patch };
            const ports = updateMockArkPorts(details, settings, buildMockPorts(details.summary.module_id), details.ports);
            details = { ...details, ports }; paint();
          }} /> : null}
        {mode === "console" || mode === "both" ? <RuntimeSurfaceWorkbench {...terminal} /> : null}
      </div>
    </I18nProvider></StrictMode>);
  }
  async function render() { await act(async () => paint()); await settleUntil(() => fixture.textContent!.includes("集群地图") || mode === "console" || fixture.querySelector(".instance-port-fields"), "Workspace did not load"); }

  for (const [edition, primary, secondary] of [["arksurvivalevolved", "TheIsland", "ScorchedEarth_P"], ["arksurvivalascended", "TheIsland_WP", "ScorchedEarth_WP"]]) {
    settings = { map_name: primary, additional_maps: [], rcon_enabled: true, admin_password: "fixture-admin-password" };
    details = { ...details, summary: { ...details.summary, module_id: edition, status: "Stopped", active_process_count: 0 }, settings_json: JSON.stringify(settings), ports: buildMockPorts(edition), active_run: null };
    await render();
    const picker = element<HTMLSelectElement>(".ark-map-editor__add select");
    check([...picker.options].some((option) => option.value === primary && option.textContent?.includes("孤岛")), `${edition}: Chinese native suggestions missing`);
    await change(picker, secondary); await click(addButton());
    check(maps().length === 1 && maps()[0].map_name === secondary, `${edition}: add did not patch native map`);
    check(/^[a-z0-9][a-z0-9-]{0,31}$/.test(maps()[0].id), "Map identity is unsafe");
    const firstId = maps()[0].id;
    details = { ...details, settings_json: JSON.stringify(settings) }; await render();
    check(!additionalRows()[0].querySelector('td:nth-child(2) input'), "Saved map package must be read-only");
    await change(additionalRows()[0].querySelector<HTMLInputElement>("th input")!, "焦土世界");
    check(maps()[0].name === "焦土世界" && maps()[0].id === firstId, "Renaming changed save identity");
    await click(additionalRows()[0].querySelector<HTMLInputElement>('input[type="checkbox"]')!);
    check(maps()[0].enabled === false && maps()[0].map_name === secondary, "Pausing a map discarded configuration");
    await click(additionalRows()[0].querySelector<HTMLInputElement>('input[type="checkbox"]')!);
    await change(element<HTMLSelectElement>(".ark-map-editor__add select"), "__custom__");
    const addInputs = () => [...fixture.querySelectorAll<HTMLInputElement>(".ark-map-editor__add input")];
    await change(addInputs()[0], "Unsafe?Port=1"); await click(addButton());
    check(maps().length === 1 && element(".ark-map-editor [role='alert']").textContent?.includes("地图包标识"), "Invalid custom map was accepted");
    await change(addInputs()[0], "CustomIsland_WP"); await change(addInputs()[1], "自定义群岛"); await click(addButton());
    check(maps().length === 2 && maps()[1].map_name === "CustomIsland_WP", "Custom package did not reach settings patch");
    await click(additionalRows()[0].querySelector<HTMLButtonElement>("button")!);
    check(element(".inline-confirm-message").textContent?.includes("地图存档会保留"), "Removal must explain retained saves");
    await click(element<HTMLButtonElement>(".inline-confirm-submit"));
    check(maps().length === 1 && maps()[0].name === "自定义群岛", "Removal changed another map");
    await change(element<HTMLSelectElement>(".ark-map-editor__add select"), secondary); await click(addButton());
    check(maps()[1].id !== firstId, "A removed map's save identity was reused");
    details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 3 } }; await render();
    check([...fixture.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLButtonElement>(".ark-map-editor input, .ark-map-editor select, .ark-map-editor button")].every((control) => control.disabled), "Running map topology remains editable");
    details = { ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 }, settings_json: JSON.stringify(settings) };
    mode = "ports"; await render();
    const portInput = (name: string) => [...fixture.querySelectorAll<HTMLLabelElement>(".instance-port-fields__item")]
      .find((label) => label.querySelector(".instance-port-fields__name")?.textContent?.startsWith(`${formatInstancePortName(name)} /`))!.querySelector<HTMLInputElement>("input")!;
    const mapGame = details.ports.find((port) => port.name === `map-${maps()[0].id}-game`)!;
    check(portInput(mapGame.name).min === "1" && portInput("rcon").min === "0", `${edition}: map ports and main disabled-service contract differ`);
    await change(portInput(mapGame.name), "0");
    check(portInput(mapGame.name).getAttribute("aria-invalid") === "true" && portValidationBlocked
      && visiblePorts.find((port) => port.name === mapGame.name)?.port === mapGame.port, "Invalid map port reached the save draft");
    await change(portInput(mapGame.name), String(mapGame.port + 100));
    if (edition === "arksurvivalevolved") check(visiblePorts.find((port) => port.name === `map-${maps()[0].id}-peer`)?.port === mapGame.port + 101, "ASE map peer did not follow its game port");
    const editedGame = visiblePorts.find((port) => port.name === mapGame.name)!.port;
    settings = { ...settings, additional_maps: [...maps(), { id: "new-endpoint", map_name: primary, name: "新地图", enabled: true }] };
    details = { ...details, ports: updateMockArkPorts(details, settings, buildMockPorts(edition), details.ports), settings_json: JSON.stringify(settings) }; await render();
    check(visiblePorts.some((port) => port.name === "map-new-endpoint-game") && visiblePorts.find((port) => port.name === mapGame.name)?.port === editedGame,
      "Map registration acknowledgement discarded a newer endpoint edit");
    mode = "maps";
    editions++;
  }

  settings = { map_name: "TheIsland_WP", rcon_enabled: true, admin_password: "fixture-admin-password", additional_maps: [
    { id: "scorched", map_name: "ScorchedEarth_WP", name: "焦土", enabled: true },
    { id: "center", map_name: "TheCenter_WP", name: "中心岛", enabled: true }
  ] satisfies AdditionalArkMap[] };
  const processes = ["main", "map-scorched", "map-center"].map((process_key, index) => ({ process_key, display_name: ["孤岛", "焦土", "中心岛"][index],
    status: "Running", is_primary: index === 0, run_id: index + 1, pid: 100 + index, log_path: `fixture/${process_key}.log` }));
  const gamePath = (process: typeof processes[number]) => `fixture/${process.process_key}-game.log`;
  const stopped = { ...details, summary: { ...details.summary, status: "Stopped" }, active_run: null, settings_json: "{}", ports: buildMockPorts(details.summary.module_id) };
  details = { ...stopped, summary: { ...details.summary, status: "Running", active_process_count: 3 }, settings_json: JSON.stringify(settings),
    active_run: { run_id: 1, process_count: 3, log_path: processes[0].log_path, processes },
    ports: updateMockArkPorts(stopped, settings, buildMockPorts(details.summary.module_id), stopped.ports) };
  Object.assign(globalThis, { isTauri: true });
  let holdCenterRead = false;
  let failCenterRead = false;
  let releaseCenterRead: (() => void) | undefined;
  let holdPrimaryRead = false;
  let releasePrimaryRead: (() => void) | undefined;
  // Replace only the native host boundary. Real controls, API serialization,
  // ReactDOM, translations, per-run log reads and event filtering remain active.
  mockIPC(async (command, args) => {
    if (command === "read_instance_log_document_from_storage") {
      const runId = Number(args.runId); reads.push(runId);
      const revision = ++documentRevision;
      sourceReads.push({ runId, source: args.source });
      if (runId === 1 && args.source === "game" && holdPrimaryRead) await new Promise<void>(resolve => { releasePrimaryRead = resolve; });
      if (runId === 3 && holdCenterRead) await new Promise<void>(resolve => { releaseCenterRead = resolve; });
      if (runId === 3 && failCenterRead) throw new Error("CENTER_LOG_READ_FAILED");
      const process = processes.find((entry) => entry.run_id === runId)!;
      if (args.source === "game") return { source_path: gamePath(process), lines: [`${process.display_name} 独立日志`, `${process.display_name} has successfully started`], total_lines: 2, truncated: false, read_error: null, snapshot_revision: revision };
      return { source_path: process.log_path, lines: [`${process.display_name} 控制台输出`], total_lines: 1, truncated: false, read_error: null };
    }
    if (command === "send_instance_runtime_command") {
      const input = args.input as Record<string, unknown>; commands.push(input);
      const process = processes.find((entry) => entry.process_key === input.processKey)!;
      return { instance_id: details.summary.id, process_key: process.process_key, display_name: process.display_name, pid: process.pid,
        command: input.command, response_text: "Fixture RCON acknowledged", write_confirmation_pending: false, submitted_at_unix_ms: 1 } satisfies InstanceRuntimeCommandResult;
    }
    throw new Error(`Unexpected native command: ${command}`);
  });
  terminal = { details, moduleDetails: buildMockModuleDetails(details.summary.module_id),
    runtime: { ...runtimeSample, recent_runs: [], diagnostics: [], health: { status: "ready", summary: "Ready" },
    log_tail: { source_path: processes[0].log_path, lines: ["孤岛 独立日志"], total_lines: 1, truncated: false, read_error: null } },
    runtimeWindows: null, startupPending: false, launchHostSurface: "managed_terminal", onSendRuntimeCommand: sendInstanceRuntimeCommand };
  fixture.classList.add("server-detail-scroll--runtime");
  fixture.style.height = "calc(100vh - 48px)";
  mode = "console"; await render();
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("孤岛 has successfully started"), "Default map source omitted the native successful-start message");
  const sourceSelect = () => element<HTMLSelectElement>(".server-runtime-console-source-select");
  check(sourceSelect().value === "game" && sourceReads.some(read => read.runId === 1 && read.source === "game"), "ARK must default to its selected map's native game log");
  check(element("pre.server-runtime-console").getAttribute("data-log-path") === gamePath(processes[0]), "Native source was not used by the console scope");
  await change(sourceSelect(), "console");
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("孤岛 控制台输出"), "Managed console source is not accessible");
  check(!element("pre.server-runtime-console").textContent!.includes("has successfully started"), "Switching sources mixed native and managed snapshots");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]), lines: [], snapshot: { source_path: gamePath(processes[0]), lines: ["native while viewing console"] }, byte_offset: 0, emitted_at_unix_ms: 2 }));
  check(!element("pre.server-runtime-console").textContent!.includes("native while viewing console"), "Native output leaked into managed source");
  holdPrimaryRead = true;
  await change(sourceSelect(), "game");
  await settleUntil(() => Boolean(releasePrimaryRead), "Primary native read did not reach its pending boundary");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]), lines: [], snapshot: { source_path: gamePath(processes[0]), lines: ["孤岛 has successfully started", "newer than pending response"] }, byte_offset: 0, emitted_at_unix_ms: 3 }));
  holdPrimaryRead = false;
  await act(async () => releasePrimaryRead!());
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("孤岛 has successfully started"), "Returning to game source lost startup output");
  check(element("pre.server-runtime-console").textContent!.includes("newer than pending response"), "A pending snapshot response overwrote a newer native document event");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]), lines: [], snapshot: { source_path: gamePath(processes[0]), lines: ["孤岛 has successfully started", "native live tail"] }, byte_offset: 0, emitted_at_unix_ms: 4 }));
  check(element("pre.server-runtime-console").textContent!.split("孤岛 has successfully started").length === 2
    && element("pre.server-runtime-console").textContent!.includes("native live tail"), "Native document replacement duplicated or lost live output");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 999, process_key: "main", log_path: gamePath(processes[0]), lines: [], snapshot: { source_path: gamePath(processes[0]), lines: ["old run wrong output"] }, byte_offset: 0, emitted_at_unix_ms: 5 }));
  check(!element("pre.server-runtime-console").textContent!.includes("old run wrong output"), "A reused native path accepted another run's event");
  const gameLines = Array.from({ length: 400 }, (_, index) => `Native line ${index}`);
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]),
    lines: [], snapshot: { source_path: gamePath(processes[0]), lines: gameLines }, byte_offset: 0, emitted_at_unix_ms: 6 }));
  const nativeConsole = () => element<HTMLPreElement>("pre.server-runtime-console");
  const distanceToBottom = () => nativeConsole().scrollHeight - nativeConsole().clientHeight - nativeConsole().scrollTop;
  check(nativeConsole().scrollHeight > nativeConsole().clientHeight && distanceToBottom() <= 1,
    `Native source must have real overflowing layout and follow its tail: ${nativeConsole().scrollHeight}/${nativeConsole().clientHeight}/${nativeConsole().scrollTop}`);
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]),
    lines: [], snapshot: { source_path: gamePath(processes[0]), lines: [...gameLines.slice(1), "Native line 400"] }, byte_offset: 0, emitted_at_unix_ms: 7 }));
  check(nativeConsole().textContent!.split("\n").length === 400 && nativeConsole().textContent!.endsWith("Native line 400") && distanceToBottom() <= 1,
    "A full native history did not retain and follow its newest line");
  await act(async () => { nativeConsole().scrollTop = 0; nativeConsole().dispatchEvent(new Event("scroll", { bubbles: true })); });
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 1, process_key: "main", log_path: gamePath(processes[0]),
    lines: [], snapshot: { source_path: gamePath(processes[0]), lines: [...gameLines.slice(2), "Native line 400", "Native line 401"] }, byte_offset: 0, emitted_at_unix_ms: 8 }));
  check(nativeConsole().scrollTop === 0 && distanceToBottom() > 48, "Native live output stole the user's scrolled reading position");
  await change(sourceSelect(), "console");
  await settleUntil(() => nativeConsole().textContent!.includes("孤岛 控制台输出"), "Source switch failed after reading historical lines");
  await change(sourceSelect(), "game");
  await settleUntil(() => nativeConsole().textContent!.includes("孤岛 has successfully started"), "Switching back to native source did not restore its document");
  const tabs = () => [...fixture.querySelectorAll<HTMLButtonElement>(".server-runtime-console-tab")];
  check(tabs().length === 3 && tabs().every((tab, index) => tab.querySelector("span")?.textContent === processes[index].display_name), "LanGameCMD must name three map tabs");
  const targets = element<HTMLSelectElement>(".server-runtime-console-target-select");
  check(targets.options.length === 3 && [...targets.options].every((option, index) => option.textContent === processes[index].display_name), "Command targets must show one option per map");
  await click(tabs()[1]);
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("焦土 独立日志"), "Secondary map log was not read");
  check(targets.value === "map-scorched" && reads.includes(2), "Map tab did not select its command target and log run");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 3, process_key: "map-center", log_path: gamePath(processes[2]), lines: [], snapshot: { source_path: gamePath(processes[2]), lines: ["wrong map output"] }, byte_offset: 0, emitted_at_unix_ms: 9 }));
  check(!element("pre.server-runtime-console").textContent!.includes("wrong map output"), "Another map's stream leaked into the selected console");
  holdCenterRead = true;
  await click(tabs()[2]);
  await settleUntil(() => Boolean(releaseCenterRead), "Center map did not hold its first native response");
  await act(async () => emitGameDocument({ instance_id: details.summary.id, run_id: 3, process_key: "map-center", log_path: gamePath(processes[2]),
    lines: [], snapshot: { source_path: gamePath(processes[2]), lines: ["中心岛 独立日志", "only center event before response"] },
    byte_offset: 0, emitted_at_unix_ms: 10 }));
  holdCenterRead = false;
  await act(async () => releaseCenterRead!());
  await settleUntil(() => nativeConsole().textContent!.includes("中心岛 独立日志"), "Center response did not settle");
  check(nativeConsole().textContent!.includes("only center event before response"),
    "Switching from another map rejected the only native event before the new map response");
  await click(tabs()[1]);
  await settleUntil(() => nativeConsole().textContent!.includes("焦土 独立日志"), "Returning to the previous map failed");
  releaseCenterRead = undefined;
  holdCenterRead = true;
  failCenterRead = true;
  await click(tabs()[2]);
  await settleUntil(() => Boolean(releaseCenterRead), "Center map did not start its held log read");
  check(targets.value === "map-center" && !element("pre.server-runtime-console").textContent!.includes("焦土 独立日志"),
    "Switching map targets displayed the previous map's output while the new read was pending");
  holdCenterRead = false;
  await act(async () => { releaseCenterRead!(); });
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("CENTER_LOG_READ_FAILED"),
    "Failed map read did not display its actual failure");
  check(!element("pre.server-runtime-console").textContent!.includes("焦土 独立日志"),
    "A failed map read left the previous map's output under the new command target");
  failCenterRead = false;
  await click(element<HTMLButtonElement>(".server-runtime-console-retry"));
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("中心岛 独立日志"),
    "Retry did not recover the selected map's own log");
  check(!element("pre.server-runtime-console").textContent!.includes("CENTER_LOG_READ_FAILED"),
    "Successful selected-map recovery retained its read error");
  await click(tabs()[1]);
  await settleUntil(() => element("pre.server-runtime-console").textContent!.includes("焦土 独立日志"),
    "Returning to Scorched Earth did not restore that map's own log");
  async function send(value: string) {
    const input = element<HTMLInputElement>(".server-runtime-console-command-input");
    await change(input, value); await act(async () => input.focus());
    await act(async () => {
      const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
      if (!response.ok) throw new Error("Native Enter dispatch failed");
    });
    await settleUntil(() => input.value === "", "Acknowledged command was not cleared");
  }
  await send("ListPlayers");
  check(commands[0].processKey === "map-scorched" && commands[0].transport === "source_rcon" && commands[0].portName === "rcon",
    "ARK command did not select the map's projected RCON endpoint");
  await click(tabs()[0]); await send("SaveWorld");
  check(commands[1].processKey === "main" && commands[1].passwordSettingKey === "admin_password" && commands[1].enabledSettingKey === "rcon_enabled", "Primary CMD did not retain the RCON credential boundary");
  async function submitCurrentDraft() {
    await act(async () => element<HTMLInputElement>(".server-runtime-console-command-input").focus());
    await act(async () => {
      const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
      if (!response.ok) throw new Error("Native Enter dispatch failed");
    });
  }
  let rejectedDispatches = 0;
  terminal = { ...terminal, onSendRuntimeCommand: async () => { rejectedDispatches++; return null; } }; await render();
  await change(element<HTMLInputElement>(".server-runtime-console-command-input"), "RetrySave"); await submitCurrentDraft();
  check(rejectedDispatches === 1 && element<HTMLInputElement>(".server-runtime-console-command-input").value === "RetrySave", "Failed command erased its draft");
  let resolveDispatch: ((result: InstanceRuntimeCommandResult | null) => void) | null = null;
  terminal = { ...terminal, onSendRuntimeCommand: () => new Promise((resolve) => { resolveDispatch = resolve; }) }; await render();
  await change(element<HTMLInputElement>(".server-runtime-console-command-input"), "DelayedSave"); await submitCurrentDraft();
  await settleUntil(() => resolveDispatch !== null, "Command did not reach the pending dispatch");
  await change(element<HTMLInputElement>(".server-runtime-console-command-input"), "NextCommand");
  await act(async () => resolveDispatch!({ instance_id: details.summary.id, process_key: "main", display_name: "孤岛", pid: 0,
    command: "DelayedSave", response_text: "Acknowledged", write_confirmation_pending: false, submitted_at_unix_ms: 2 }));
  check(element<HTMLInputElement>(".server-runtime-console-command-input").value === "NextCommand", "An earlier acknowledgement erased a newer command draft");
  await change(element<HTMLInputElement>(".server-runtime-console-command-input"), "");
  terminal = { ...terminal, onSendRuntimeCommand: sendInstanceRuntimeCommand };
  terminal = { ...terminal, details: { ...details, settings_json: JSON.stringify({ ...settings, rcon_enabled: false }) } }; await render();
  check(element<HTMLInputElement>(".server-runtime-console-command-input").disabled && element<HTMLInputElement>(".server-runtime-console-command-input").placeholder.includes("rcon_enabled"), "Disabled RCON needs an accurate configuration hint");
  terminal = { ...terminal, details: { ...details, settings_json: JSON.stringify({ ...settings, admin_password: "" }) } }; await render();
  check(element<HTMLInputElement>(".server-runtime-console-command-input").disabled && element<HTMLInputElement>(".server-runtime-console-command-input").placeholder.includes("admin_password"), "Missing RCON password must prevent command submission");
  fixture.classList.remove("server-detail-scroll--runtime");
  fixture.style.removeProperty("height");
  terminal = { ...terminal, details }; mode = "both"; await render();
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Desktop map and CMD panels overflow the viewport");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
    await act(async () => root.unmount()); await act(async () => { await Promise.resolve(); });
    if (bridge.active.size) throw new Error("Native log listeners remain after map CMD unmount");
    return { browser_errors: errors, native_dialogs: 0 };
  } });
  return { status: "passed", checks, editions, patches: patchCount, console_tabs: tabs().length, commands: commands.length, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
