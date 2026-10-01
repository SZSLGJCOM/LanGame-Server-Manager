import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { AppShell } from "../../src/components/AppShell";
import { I18nProvider } from "../../src/i18n";
import { SystemView } from "../../src/views/SystemView";
import type { SystemSnapshot } from "../../src/types";
import "../../src/app.css";

const query = new URLSearchParams(location.search);
const nonce = query.get("nonce");
if (nonce) Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const browserErrors: string[] = [];
addEventListener("error", (event) => browserErrors.push(event.message));
addEventListener("unhandledrejection", (event) => browserErrors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { browserErrors.push(args.map(String).join(" ")); originalError(...args); };

const noOperation = () => {};
const gib = 1024 ** 3;
const scenarios = ["normal", "unknown", "stale", "staleUpdating", "unknownUpdating", "freshUpdating", "memoryPressure", "diskPressure", "diskWatch", "networkWithoutLink", "hardwareFallback", "hardwareUnmatched"] as const;
type Scenario = typeof scenarios[number];
type FixtureConfig = { locale: "zh-CN" | "en-US"; theme: "dark" | "light"; captureScenario: Scenario };
let checks = 0;
let activeScenario: Scenario | "initializing" = "initializing";
let phase = "bootstrap";

function snapshotFor(base: SystemSnapshot, scenario: Scenario): SystemSnapshot {
  // These bounded, synthetic observations exercise the real production model/UI.
  // They are not measurements of the workstation or live game availability.
  const snapshot: SystemSnapshot = {
    ...base,
    telemetry: { observed_at_unix_ms: Date.now(), cpu: "valid", cpu_cores: "valid", memory: "valid",
      disk_capacity: "valid", disk_io: "valid", network: "valid" },
    cpu_percent: 30, cpu_single_core_peak_percent: 40,
    cpu_cores: [{ name: "0", utility_percent: 40, performance_percent: 100, frequency_mhz: 3500 }],
    memory_percent: 50, memory_total_bytes: 32 * gib, memory_available_bytes: 16 * gib,
    memory_commit_used_bytes: 20 * gib, memory_commit_limit_bytes: 64 * gib,
    memory_modules: [{ bank_label: "BANK 0", device_locator: "DIMM 0", manufacturer: "Kingston",
      part_number: "KF560C36-16", capacity_bytes: 32 * gib, speed_mts: 6000, configured_clock_mts: 4800,
      configured_voltage_mv: 1100, memory_type: "DDR5", timing_summary: "" }],
    disk_used_percent: 70, disk_total_bytes: 1000 * gib, disk_used_bytes: 700 * gib,
    disk_label: "\\\\?\\C:\\", disk_volume_id: null, disk_model: "Samsung SSD 990 PRO 1TB",
    disk_read_bps: 0, disk_write_bps: 0, disk_read_latency_ms: 0, disk_write_latency_ms: 0, disk_queue_length: 0,
    disk_volumes: [
      { id: "C:\\", label: "C:", paths: ["C:\\LanGame\\runtime"], total_bytes: 1000 * gib,
        available_bytes: 300 * gib, free_bytes: 300 * gib, status: "valid" },
      { id: "D:\\", label: "D:", paths: ["D:\\LanGame\\worlds"], total_bytes: 100 * gib,
        available_bytes: 30 * gib, free_bytes: 30 * gib, status: "valid" }
    ],
    network_receive_bps: 0, network_transmit_bps: 0,
    network_adapters: [{ name: "Fixture Ethernet", description: "Synthetic adapter", status: "Up", rate_status: "valid",
      ipv4_addresses: [], link_speed_bps: 1_000_000_000, received_bytes: 0, transmitted_bytes: 0,
      receive_bps: 0, transmit_bps: 0 }]
  };
  if (scenario === "unknown" || scenario === "unknownUpdating") {
    snapshot.telemetry = { observed_at_unix_ms: scenario === "unknownUpdating" ? Date.now() : null, cpu: "warming_up", cpu_cores: "warming_up",
      memory: "unavailable", disk_capacity: "unavailable", disk_io: "unavailable", network: "unavailable" };
    snapshot.disk_volumes = snapshot.disk_volumes!.map((volume) => ({ ...volume, status: "unavailable" }));
  } else if (scenario === "stale" || scenario === "staleUpdating") {
    snapshot.telemetry = { ...snapshot.telemetry!, observed_at_unix_ms: Date.now() - 240_000 };
  } else if (scenario === "memoryPressure") {
    snapshot.memory_available_bytes = 128 * 1024 ** 2;
    snapshot.memory_percent = 99.6;
    snapshot.memory_commit_used_bytes = Math.round(63.9 * gib);
  } else if (scenario === "diskPressure") {
    // The healthy larger volume must never hide an exhausted business volume.
    snapshot.disk_volumes![1] = { ...snapshot.disk_volumes![1], available_bytes: 64 * 1024 ** 2,
      free_bytes: 64 * 1024 ** 2 };
  } else if (scenario === "networkWithoutLink") {
    snapshot.network_receive_bps = 4 * 1024 ** 2;
    snapshot.network_transmit_bps = 2 * 1024 ** 2;
    snapshot.network_adapters![0] = { ...snapshot.network_adapters![0], link_speed_bps: 0,
      receive_bps: snapshot.network_receive_bps, transmit_bps: snapshot.network_transmit_bps };
  } else if (scenario === "hardwareFallback") {
    snapshot.memory_modules = null;
    snapshot.disk_model = null;
    snapshot.disk_volume_id = "D:\\";
    snapshot.disk_label = "\\\\?\\C:\\";
  } else if (scenario === "hardwareUnmatched") {
    snapshot.disk_volume_id = "unmatched-volume";
    snapshot.disk_label = "\\\\?\\C:\\";
    snapshot.disk_model = "Different disk model";
  } else if (scenario === "diskWatch") {
    snapshot.disk_volumes![0] = { ...snapshot.disk_volumes![0], total_bytes: 10 * gib,
      available_bytes: 5.5 * gib, free_bytes: 5.5 * gib };
    snapshot.disk_volumes![1] = { ...snapshot.disk_volumes![1], total_bytes: 200 * gib,
      available_bytes: 6 * gib, free_bytes: 6 * gib };
  }
  return snapshot;
}

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`[${activeScenario}; ${phase}; checks=${checks}] ${description}`);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = document.querySelector<T>(selector);
  check(selected, `Missing ${selector}`);
  return selected;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function key(name: "Tab" | "Enter" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native browser key ${name} must dispatch`);
  });
}
function text(selector: string) { return element(selector).textContent?.trim() ?? ""; }
function geometry(target: HTMLElement) {
  const box = target.getBoundingClientRect();
  return {
    class: target.className, text: target.textContent?.trim(),
    rect: { x: box.x, y: box.y, width: box.width, height: box.height },
    client: { width: target.clientWidth, height: target.clientHeight },
    scroll: { width: target.scrollWidth, height: target.scrollHeight },
    opacity: getComputedStyle(target).opacity
  };
}
async function settleCoreDrawing(activeTone: string | null = null) {
  const deadline = performance.now() + 5000;
  let previous: number[] | null = null;
  let stableFrames = 0;
  while (stableFrames < 4) {
    check(performance.now() < deadline, "Core opacity and geometry must settle before inspection or capture");
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
    const targets = [...document.querySelectorAll<HTMLElement>(
      ".system-core-parallax, .system-core-dial, .system-core-center, .system-core-center strong, .system-core-metric"
    )];
    check(targets.length >= 8, "Core capture requires its dial, center, and four channels");
    const ready = targets.every((target) => {
      // A selected channel deliberately dims the other three to 0.46.
      const dimmed = activeTone && target.classList.contains("system-core-metric")
        && !target.classList.contains(`system-core-metric--${activeTone}`);
      return Math.abs(Number(getComputedStyle(target).opacity) - (dimmed ? 0.46 : 1)) < 0.001;
    });
    const current = targets.flatMap((target) => {
      const box = target.getBoundingClientRect();
      return [box.x, box.y, box.width, box.height];
    });
    const unchanged = previous?.length === current.length
      && current.every((value, index) => Math.abs(value - previous![index]) < 0.1);
    stableFrames = ready && unchanged ? stableFrames + 1 : 0;
    previous = current;
  }
}
function assertCoreGeometry() {
  const buttons = [...document.querySelectorAll<HTMLElement>(".system-core-metric")];
  const center = element(".system-core-center").getBoundingClientRect();
  const overlaps = (left: DOMRect, right: DOMRect) =>
    Math.min(left.right, right.right) - Math.max(left.left, right.left) > 1
    && Math.min(left.bottom, right.bottom) - Math.max(left.top, right.top) > 1;
  check(buttons.length === 4, "The dial must expose four independent resource channels");
  for (let index = 0; index < buttons.length; index++) {
    for (const other of buttons.slice(index + 1)) {
      check(!overlaps(buttons[index].getBoundingClientRect(), other.getBoundingClientRect()),
        `Dial resource buttons must not overlap: ${JSON.stringify({
          left: geometry(buttons[index]), right: geometry(other), stage: geometry(element(".system-core-stage"))
        })}`);
    }
    const bounds = buttons[index].getBoundingClientRect();
    for (const target of buttons[index].querySelectorAll<HTMLElement>(
      ".system-core-metric-label, .system-core-metric-value, .system-core-metric-value strong, .system-core-metric-value em, .system-core-metric-live"
    )) {
      const box = target.getBoundingClientRect();
      check(box.left >= bounds.left - 1 && box.right <= bounds.right + 1 && box.top >= bounds.top - 1 && box.bottom <= bounds.bottom + 1,
        `Channel text must fit the metric button: ${JSON.stringify({ text: geometry(target), button: geometry(buttons[index]) })}`);
      check(target.scrollWidth <= target.clientWidth + 1,
        `Channel labels and readings must not be horizontally clipped: ${JSON.stringify(geometry(target))}`);
    }
  }
  for (const target of document.querySelectorAll<HTMLElement>(
    ".system-core-center-kicker, .system-core-center-value, .system-core-center-value strong, .system-core-center-state"
  )) {
    const box = target.getBoundingClientRect();
    check(box.width > 0 && box.height > 0 && box.left >= center.left - 1 && box.right <= center.right + 1
      && box.top >= center.top - 1 && box.bottom <= center.bottom + 1,
    `Center text must fit its available region: ${JSON.stringify({ text: geometry(target), center: geometry(element(".system-core-center")) })}`);
    check(target.scrollWidth <= target.clientWidth + 1 && target.scrollHeight <= target.clientHeight + 1,
      `Center text must not be clipped: ${JSON.stringify({ text: geometry(target), center: geometry(element(".system-core-center")) })}`);
    check(buttons.every((button) => !overlaps(box, button.getBoundingClientRect())),
      `Resource buttons must not obscure center text: ${JSON.stringify({ text: geometry(target), buttons: buttons.map(geometry) })}`);
  }
}
function assertResourceLayout() {
  const panel = element(".system-core-panel");
  const summary = element(".system-resource-summary");
  const stage = element(".system-core-stage");
  const bounds = panel.getBoundingClientRect();
  const summaryBounds = summary.getBoundingClientRect();
  check(bounds.width > 0 && bounds.height > 0, "Resource panel must be rendered");
  check(summaryBounds.top >= stage.getBoundingClientRect().bottom - 1, "Resource footer must not overlap the core dial");
  check(summaryBounds.left >= bounds.left - 1 && summaryBounds.right <= bounds.right + 1
    && summaryBounds.bottom <= bounds.bottom + 1, "Resource footer must fit its panel");
  for (const target of [summary, ...summary.querySelectorAll<HTMLElement>("p, time, dt, dd")]) {
    const box = target.getBoundingClientRect();
    check(box.width > 0 && box.height > 0 && box.left >= bounds.left - 1 && box.right <= bounds.right + 1,
      `Resource copy must fit horizontally: ${target.textContent}`);
    check(target.scrollWidth <= target.clientWidth + 1 && target.scrollHeight <= target.clientHeight + 1,
      `Resource copy must not be clipped: ${target.textContent}`);
  }
  check(document.documentElement.scrollWidth <= innerWidth + 1, "System dashboard must not overflow the window");
  check(!summary.querySelector("details, summary, button"), "Resource headroom must remain static in the existing footer");
  check(summary.querySelectorAll(".system-resource-headroom > div").length === 3,
    "The compact footer must show memory, commitment and the tightest business volume");
  const shell = element(".shell-content-scroll");
  check(getComputedStyle(shell).overflowY === (innerHeight <= 740 ? "auto" : "hidden"),
    "Resource data must preserve the original desktop shell and its existing short-viewport scrolling");
  const grid = element(".system-command-grid").getBoundingClientRect();
  const content = element(".shell-content-body").getBoundingClientRect();
  check(grid.bottom <= content.bottom + 1, "The original dashboard grid must remain inside the fixed shell");
  return { panel_width: bounds.width, panel_height: bounds.height, footer_height: summaryBounds.height };
}
function assertNoScoreClaims() {
  const copy = text(".system-core-panel");
  check(!/健康指数|安全边界|Health index|Safe margin/i.test(copy), "Resource panel must not retain health scores or safe margin claims");
  check(!/适合继续开服|可以继续开服|ready to (?:start|launch)|room for more servers/i.test(copy),
    "Host resource state must not promise that more game servers can run");
  check(!/\d|%/.test(text(".system-core-center-value")), "Unselected center must be a resource state, not a numeric score");
}

async function mount() {
  await act(prepareBrowserLocaleCatalogs);
  const config: FixtureConfig = nonce
    ? await fetch("/__system_resources_config").then((response) => {
      check(response.ok, "System resource configuration must load");
      return response.json() as Promise<FixtureConfig>;
    })
    : { locale: query.get("locale") === "en-US" ? "en-US" : "zh-CN",
      theme: query.get("theme") === "light" ? "light" : "dark", captureScenario: "normal" };
  localStorage.setItem("langame.locale", config.locale);
  document.documentElement.dataset.theme = config.theme;
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: true });
  check(bootstrap.state.snapshot, "System browser fixture requires a snapshot");
  const root = createRoot(element("#fixture"));
  const props: ComponentProps<typeof SystemView> = {
    snapshot: bootstrap.state.snapshot, instances: bootstrap.state.instances.slice(0, 2),
    bindAddressCandidates: [], appSettings: bootstrap.state.settings,
    steamCmdStatus: null, steamCmdBusy: false, steamCmdProgress: null, steamCmdMessage: "",
    onOpenInstance: noOperation, onPickDirectory: async () => null, onSaveAppSettings: noOperation,
    onEnsureSteamCmd: noOperation, onUninstallSteamCmd: noOperation
  };
  const aiSettings = createDefaultAiSettings();
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView: "system", theme: config.theme, serverCount: props.instances.length, aiSettings, activityText: "",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "Assistant", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale: config.locale, activeJobsCount: 0, activeView: "system", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
      selectedInstanceDetails: null, selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
      selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
      selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: noOperation, onThemeChange: noOperation
  };
  const render = async (scenario: Scenario, refreshing = scenario.endsWith("Updating"), snapshot = snapshotFor(props.snapshot, scenario)) => {
    activeScenario = scenario;
    phase = "render";
    await act(async () => { root.render(<StrictMode><I18nProvider><AppShell {...shellProps}>
      <SystemView key={scenario} {...props} snapshot={snapshot} systemRefreshing={refreshing} />
    </AppShell></I18nProvider></StrictMode>); });
    return snapshot;
  };
  if (!nonce) { await render(config.captureScenario); return; }
  await render("normal");
  await settleUntil(() => Boolean(document.querySelector(".system-resource-summary")), "Resource summary did not mount");
  await settleUntil(() => document.querySelector(".shell-locale-button")?.textContent === (config.locale === "en-US" ? "ZH" : "EN"),
    "Requested interface language did not load");
  await act(async () => { await document.fonts.ready; });
  const labels = config.locale === "zh-CN"
    ? { normal: "正常", unknown: "数据不足", stale: "数据过期", updating: "更新中…", pressure: "紧张", watch: "需关注", memory: /内存|提交/, disk: /磁盘|卷/ }
    : { normal: "Normal", unknown: "Incomplete", stale: "Stale", updating: "Updating…", pressure: "Critical", watch: "Attention", memory: /memory|commit/i, disk: /disk|volume|free space/i };
  const observed: Partial<Record<Scenario, { state: string; reason: string }>> = {};
  for (const scenario of scenarios) {
    const renderedSnapshot = await render(scenario);
    const updating = scenario === "staleUpdating" || scenario === "unknownUpdating";
    const expected = updating ? labels.updating : scenario === "memoryPressure" || scenario === "diskPressure" ? labels.pressure
      : scenario === "networkWithoutLink" || scenario === "hardwareFallback" || scenario === "hardwareUnmatched" || scenario === "freshUpdating" ? labels.normal : scenario === "diskWatch" ? labels.watch : labels[scenario as "normal" | "unknown" | "stale"];
    await settleUntil(() => text(".system-core-center-value") === expected, `${scenario} should show ${expected}`);
    const reason = text(".system-resource-reason");
    check(reason.length > 0, `${scenario} requires an actionable or explanatory reason`);
    check(element(".system-core-visual").getAttribute("aria-label")?.includes(expected), "Accessible label must include resource state");
    if (updating) {
      check(text(".system-resource-reason") === labels.updating, "Active requests must replace the existing stale or unknown footer status in place");
      check(element(".system-core-stage").getAttribute("aria-busy") === "true", "The pending stale or unknown request must expose busy state");
      check(!document.querySelector(".system-core-center-state"), "Updating must not add a duplicate center subline");
      check(element<HTMLTimeElement>(".system-resource-sample time").dateTime === new Date(renderedSnapshot.telemetry!.observed_at_unix_ms!).toISOString(),
        "Updating must retain the observation timestamp rather than claim a new sample");
    } else if (scenario === "freshUpdating") {
      check(!text(".system-core-panel").includes(labels.updating), "A fresh background refresh must leave the current resource state readable");
      check(text(".system-core-metric--cpu .system-core-metric-value") === "30%", "A fresh background refresh must preserve current measurements");
    }
    if (scenario === "normal") {
      check(!document.querySelector(".system-core-center-state"), "The normal unselected center must not repeat Sampled");
      check(text(".system-top-metric--memory .system-top-metric-channel") === "Kingston KF560C36-16",
        "The memory card must show the actual manufacturer and part number");
      check(text(".system-top-metric--memory .system-top-metric-detail") === "4800 MT/s · 16 GiB / 32 GiB",
        "The memory card must show configured MT/s with used and total capacity in its existing detail line");
      check(text(".system-top-metric--disk .system-top-metric-channel") === "Samsung SSD 990 PRO 1TB",
        "The disk card must show the physical model associated with its displayed primary volume");
    } else if (scenario === "hardwareFallback") {
      check(text(".system-top-metric--disk .system-top-metric-channel") === "D:",
        "A missing physical model must use the clean volume label selected by stable ID before a stale drive label");
      check(!/MT\/s|Kingston/.test(text(".system-top-metric--memory")), "Absent memory hardware must not fabricate a model or speed");
    } else if (scenario === "hardwareUnmatched") {
      check(text(".system-top-metric--disk .system-top-metric-channel") === "C:",
        "An unmatched stable ID must not assign its model to a different volume reusing the same drive label");
      check(!/Volume\{|Different disk/.test(text(".system-top-metric--disk")), "Internal IDs and unmatched disk models must remain hidden");
    }
    if (scenario === "unknown" || scenario === "stale" || updating) {
      if (!updating) check(Boolean(document.querySelector(".system-core-center-state")), `${scenario} must retain its center sample status`);
      const values = [...document.querySelectorAll(".system-core-metric-value")].map((node) => node.textContent?.trim());
      check(values.length === 4 && values.every((value) => value === "—"), `${scenario} metrics must not display stale numbers or zero: ${values}`);
      check(document.querySelectorAll(".system-core-band-progress").length === 0, `${scenario} must not render fabricated resource arcs`);
    } else if (scenario === "memoryPressure") {
      check(labels.memory.test(reason), "Memory pressure must identify memory or commitment");
    } else if (scenario === "diskPressure") {
      check((labels.disk.test(reason) || /可用空间/.test(reason)) && /D:/.test(reason), "Disk pressure must name the exhausted D: business volume");
    } else if (scenario === "diskWatch") {
      check(/D:/.test(text(".system-resource-headroom")), "The watched D: volume must outrank a healthy C: volume with fewer available bytes");
    } else if (scenario === "networkWithoutLink") {
      await settleCoreDrawing();
      // A second real observation is required before a history line can exist.
      await render(scenario);
      const network = element<HTMLButtonElement>(".system-core-metric--network");
      check(/6\s*MiB\/s/.test(text(".system-top-metric--network .system-top-metric-value")),
        "Valid host throughput must remain visible when adapter link speed is unknown");
      await settleUntil(() => Boolean(document.querySelector(".system-top-metric--network .system-sparkline-line[points]:not([points=''])")),
        "Real RX/TX history must remain visible without an adapter link-speed denominator");
      check(!network.querySelector(".is-unknown"), "Unknown link speed must not invalidate measured host traffic");
      await act(async () => { network.click(); });
      check(/6\s*MiB\/s/.test(text(".system-core-center-value")), "Mouse-selected network must show measured throughput");
      await settleCoreDrawing("network");
      assertCoreGeometry();
      await act(async () => { network.click(); network.focus(); });
      await key("Enter");
      check(network.getAttribute("aria-pressed") === "true" && /6\s*MiB\/s/.test(text(".system-core-center-value")),
        "Keyboard-selected network must show measured throughput");
      await settleCoreDrawing("network");
      assertCoreGeometry();
      await key("Escape");
      await act(async () => { network.blur(); });
    }
    phase = "scenario geometry";
    await settleCoreDrawing();
    assertCoreGeometry();
    assertNoScoreClaims();
    assertResourceLayout();
    observed[scenario] = { state: text(".system-core-center-value"), reason };
    if (updating) {
      phase = "failed refresh recovery";
      await render(scenario, false, renderedSnapshot);
      const failedState = scenario === "staleUpdating" ? labels.stale : labels.unknown;
      check(text(".system-core-center-value") === failedState && !text(".system-resource-reason").includes(labels.updating),
        "A completed failed request must restore the underlying stale or unknown status");
      check(element<HTMLTimeElement>(".system-resource-sample time").dateTime === new Date(renderedSnapshot.telemetry!.observed_at_unix_ms!).toISOString(),
        "A failed request must preserve the original observation time");
      await settleCoreDrawing();
      assertCoreGeometry();
      phase = "successful refresh recovery";
      await render(scenario, false, snapshotFor(props.snapshot, "normal"));
      check(text(".system-core-center-value") === labels.normal && text(".system-core-metric--cpu .system-core-metric-value") === "30%",
        "A successful new sample must restore normal resource readings");
    }
    checks++;
  }
  await render("normal");
  phase = "mouse selection";
  const cpu = element<HTMLButtonElement>(".system-core-metric--cpu");
  await act(async () => { cpu.click(); });
  check(cpu.getAttribute("aria-pressed") === "true", "Mouse selection must pin CPU");
  check(text(".system-core-center-state") === (config.locale === "zh-CN" ? "锁定" : "LOCKED"),
    "Pinned selections must retain their state indicator");
  check(text(".system-core-center-value") === "30%", "Pinned CPU must show its measured value");
  await act(async () => { cpu.click(); });
  check(cpu.getAttribute("aria-pressed") === "false", "Selecting CPU twice must release it");
  check(document.querySelectorAll(".system-top-metric").length === 4, "All four resource cards must remain available");
  const diskCard = element<HTMLButtonElement>(".system-top-metric--disk");
  await act(async () => { diskCard.click(); });
  check(element(".system-core-metric--disk").getAttribute("aria-pressed") === "true",
    "Selecting a resource card must pin the corresponding dial channel");
  await act(async () => { diskCard.click(); });
  checks++;
  phase = "keyboard selection";
  await act(async () => { cpu.focus(); });
  await key("Tab");
  const memory = element<HTMLButtonElement>(".system-core-metric--memory");
  check(document.activeElement === memory, "Keyboard Tab must reach the memory channel");
  await key("Enter");
  check(memory.getAttribute("aria-pressed") === "true", "Keyboard Enter must pin the focused channel");
  await key("Escape");
  check(memory.getAttribute("aria-pressed") === "false", "Escape must release a pinned channel");
  await act(async () => { memory.blur(); });
  checks++;
  await render(config.captureScenario);
  phase = "static capacity footer";
  const headroom = element(".system-resource-headroom");
  const volumeItem = headroom.querySelector<HTMLElement>("[title]");
  check(/C:/.test(volumeItem?.title ?? "") && /D:/.test(volumeItem?.title ?? ""), "The volume tooltip must retain all business volumes");
  check(!headroom.querySelector("details, summary, button"), "Resource headroom must not require expansion");
  check(!/实例可用性需|启动新实例仍需|Check each instance|Check the selected game/.test(text(".system-resource-summary")),
    "The footer must not add unrelated explanatory prose");
  phase = "final capture";
  await settleCoreDrawing();
  assertCoreGeometry();
  const layout = assertResourceLayout();
  assertNoScoreClaims();
  if (config.captureScenario === "networkWithoutLink") {
    await act(async () => { element<HTMLButtonElement>(".system-core-metric--network").click(); });
    await settleCoreDrawing("network");
    assertCoreGeometry();
  }
  check(browserErrors.length === 0, `Unexpected browser errors: ${browserErrors.join("; ")}`);
  checks++;
  return { status: "passed", checks, ...config, scenarios: observed, layout,
    interaction: { mouse_selection: true, keyboard_selection: true, escape_release: true },
    data_source: "synthetic snapshots rendered by production SystemView", browser_errors: browserErrors };
}

if (nonce) {
  let watchdog: ReturnType<typeof setTimeout>;
  void Promise.race([mount(), new Promise<never>((_, reject) => {
    watchdog = setTimeout(() => reject(new Error(`System resource checks stalled after ${checks} checks`)), 25000);
  })]).finally(() => {
    clearTimeout(watchdog);
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  }).catch((error) => ({ status: "failed", checks, scenario: activeScenario, phase,
    error: error instanceof Error ? error.stack : String(error), browser_errors: browserErrors }))
    .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
} else {
  void mount();
}
