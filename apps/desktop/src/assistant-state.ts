import type { LibraryPageMode, RuntimeRefreshIssue } from "./app-state";
import { getAiSettingsStatus, type AiSettings, type AiSettingsMissingField } from "./ai-settings";
import { assistantIgnoresActivePortPreflight, buildAssistantLaunchIssues, selectedAssistantLaunchPlan } from "./assistant-launch-diagnostics";
import type { TranslateFn } from "./i18n";
import { selectLocaleText } from "./i18n-config";
import { localizeRuntimeHealthSummary } from "./runtime-health-message";
import type {
  BootstrapResponse,
  InstanceDetails,
  InstanceRuntimeOverview,
  LaunchPlan,
  LogTailSnapshot,
  ModuleDetails,
  ServerWorkspaceSection,
  SteamCmdStatus,
  ViewKey
} from "./types";

export type AssistantSeverity = "info" | "warning" | "critical";

export type AssistantActionId =
  | "ensure-storage"
  | "ensure-steamcmd"
  | "resume-runtime-refresh"
  | "view-system"
  | "open-ai-settings"
  | "view-library"
  | "view-servers"
  | "refresh-launch-preview";

export interface AssistantAction {
  id: AssistantActionId;
  label: string;
}

export interface AssistantIssue {
  id: string;
  severity: AssistantSeverity;
  title: string;
  detail: string;
  action?: AssistantAction | null;
}

export interface AssistantPromptCard {
  id: string;
  label: string;
  preview: string;
  prompt: string;
  payload: string;
}

type AssistantPromptCards = [
  AssistantPromptCard,
  AssistantPromptCard,
  AssistantPromptCard,
  AssistantPromptCard
];

export interface AssistantViewModel {
  closeLabel: string;
  issues: AssistantIssue[];
  panelTitle: string;
  prompts: AssistantPromptCard[];
  contextPayload: string;
}

export interface AssistantBuildInput {
  aiSettings: AiSettings;
  locale: string;
  activeJobsCount: number;
  activeView: ViewKey;
  bootstrap: BootstrapResponse;
  storageReady: boolean;
  libraryPage: LibraryPageMode;
  overlayNames: string[];
  runtimeAutoRefreshPaused: boolean;
  runtimeRefreshIssue: RuntimeRefreshIssue | null;
  selectedInstanceDetails: InstanceDetails | null;
  selectedInstanceId: string | null;
  selectedInstanceModuleDetails: ModuleDetails | null;
  selectedLaunchPlan: LaunchPlan | null;
  selectedLaunchPlanError: string | null;
  selectedLogDocument: LogTailSnapshot | null;
  selectedModuleDetails: ModuleDetails | null;
  selectedRuntime: InstanceRuntimeOverview | null;
  serverWorkspaceSection: ServerWorkspaceSection;
  steamCmdStatus: SteamCmdStatus | null;
}

interface AssistantCopy {
  closeLabel: string;
  panelTitle: string;
  viewSystem: string;
  viewLibrary: string;
  viewLibraryDetail: string;
  viewServers: string;
  viewServerSettings: string;
  issueStorageTitle: string;
  issueStorageDetail: string;
  issuePollingTitle: string;
  issuePollingDetail: string;
  issueRuntimeErrorTitle: string;
  issueRuntimeWarningTitle: string;
  issueRuntimeErrorDetail: (summary: string) => string;
  issueRuntimeWarningDetail: (summary: string) => string;
  issueSteamCmdTitle: string;
  issueSteamCmdDetail: string;
  issueJobsTitle: (count: number) => string;
  issueJobsDetail: string;
  promptLogsLabel: string;
  promptLogsPreview: string;
  promptLogsBody: (instanceName: string, summary: string) => string;
  promptNetworkLabel: string;
  promptNetworkPreview: string;
  promptNetworkBody: (instanceName: string) => string;
  promptSettingsLabel: string;
  promptSettingsPreview: string;
  promptSettingsBody: (instanceName: string) => string;
  promptModuleLabel: string;
  promptModulePreview: string;
  promptModuleBody: (moduleName: string) => string;
  promptInstallLabel: string;
  promptInstallPreview: string;
  promptInstallBody: (moduleName: string) => string;
  promptSystemLabel: string;
  promptSystemPreview: string;
  promptSystemBody: string;
  promptOverviewLabel: string;
  promptOverviewPreview: string;
  promptOverviewBody: string;
  promptOptimizationLabel: string;
  promptOptimizationPreview: string;
  promptOptimizationBody: string;
  promptNextStepsLabel: string;
  promptNextStepsPreview: string;
  promptNextStepsBody: string;
  exportInstructions: string[];
  actionLabels: Record<AssistantActionId, string>;
  aiFieldLabels: Record<AiSettingsMissingField, string>;
}

const zhCn: AssistantCopy = {
  closeLabel: "关闭 LAN",
  panelTitle: "LAN",
  viewSystem: "系统",
  viewLibrary: "游戏库",
  viewLibraryDetail: "游戏库 / 详情",
  viewServers: "服务器",
  viewServerSettings: "服务器 / 配置",
  issueStorageTitle: "存储尚未初始化",
  issueStorageDetail: "数据库或迁移还没有准备好，后续 AI 诊断会缺少完整的实例与运行记录。",
  issuePollingTitle: "自动刷新已暂停",
  issuePollingDetail: "运行态轮询在连续失败后被暂停，日志和健康状态可能已经落后于真实情况。",
  issueRuntimeErrorTitle: "当前实例存在错误信号",
  issueRuntimeWarningTitle: "当前实例存在预警信号",
  issueRuntimeErrorDetail: (summary) => `运行态已经给出错误结论：${summary}`,
  issueRuntimeWarningDetail: (summary) => `最近日志里已经出现预警迹象：${summary}`,
  issueSteamCmdTitle: "SteamCMD 尚未就绪",
  issueSteamCmdDetail: "游戏安装 / 更新链路还未准备好，模块安装与校验会被卡住。",
  issueJobsTitle: (count) => `${count} 个后台任务仍在运行`,
  issueJobsDetail: "安装、校验或运行任务仍在推进中，建议优先查看运行态和输出。",
  promptLogsLabel: "诊断当前日志",
  promptLogsPreview: "分析为什么启动失败、卡启动或异常退出",
  promptLogsBody: (instanceName, summary) =>
    `请结合下面的服务器上下文，分析实例“${instanceName}”为什么会处于当前运行状态。先给结论，再列证据，最后给出最短排查路径。当前健康摘要：${summary}`,
  promptNetworkLabel: "排查别人连不上",
  promptNetworkPreview: "检查 bind IP、端口和联机入口",
  promptNetworkBody: (instanceName) =>
    `请根据下面的上下文，判断为什么其他玩家可能连不上实例“${instanceName}”。重点检查 bind IP、端口、实例状态、overlay 提示和启动信号，并按概率从高到低给出排查顺序。`,
  promptSettingsLabel: "审查当前配置",
  promptSettingsPreview: "检查 settings_json 和 schema 风险点",
  promptSettingsBody: (instanceName) =>
    `请根据下面的上下文审查实例“${instanceName}”的当前配置，指出容易导致启动失败、无法联机或维护成本升高的字段，并给出更稳妥的默认建议。`,
  promptModuleLabel: "设计默认开服方案",
  promptModulePreview: "基于模块信息给出适合小团队的默认建议",
  promptModuleBody: (moduleName) =>
    `请根据下面的模块信息，为“${moduleName}”设计一套适合局域网或小团队联机的默认开服方案，并说明关键字段为什么要这样设置。`,
  promptInstallLabel: "解释安装链路",
  promptInstallPreview: "分析 SteamCMD、安装目录和前置条件",
  promptInstallBody: (moduleName) =>
    `请根据下面的模块与安装上下文，解释“${moduleName}”当前的安装链路、常见失败点和最稳妥的检查步骤。`,
  promptSystemLabel: "总结当前系统风险",
  promptSystemPreview: "总结主机和实例里最该优先处理的问题",
  promptSystemBody:
    "请总结下面这台主机和项目上下文里最值得优先处理的 3 件事，按影响面和修复顺序排序，并说明每一项为什么重要。",
  promptOverviewLabel: "分析当前状态",
  promptOverviewPreview: "快速看清当前页面的重要信息和异常信号",
  promptOverviewBody:
    "请根据下面的当前页面信息，概括整体状态、关键数据和异常信号。先给简明结论，再说明最值得关注的内容。",
  promptOptimizationLabel: "给出优化建议",
  promptOptimizationPreview: "从稳定性、性能和维护成本三个方面改进",
  promptOptimizationBody:
    "请根据下面的主机和项目上下文，从稳定性、性能与维护成本三个方面给出优化建议，并按投入产出比排序。",
  promptNextStepsLabel: "规划下一步",
  promptNextStepsPreview: "按优先级整理可执行的后续操作",
  promptNextStepsBody:
    "请根据下面的当前上下文规划下一步操作，按优先级列出可执行事项，并标明每一步的目标与完成标准。",
  exportInstructions: [
    "请只基于给出的上下文判断，不要假设未提供的信息。",
    "回答时先给结论，再给证据，最后给下一步。",
    "如果涉及风险，请明确指出最可能的根因和成本最低的验证方法。"
  ],
  actionLabels: {
    "ensure-storage": "初始化存储",
    "ensure-steamcmd": "准备 SteamCMD",
    "resume-runtime-refresh": "恢复自动刷新",
    "view-system": "打开系统页",
    "open-ai-settings": "配置 BYOK",
    "view-library": "打开游戏库",
    "view-servers": "打开服务器页",
    "refresh-launch-preview": "刷新启动预览"
  },
  aiFieldLabels: {
    model: "模型",
    baseUrl: "Base URL",
    apiKey: "API Key"
  }
};

const enUs: AssistantCopy = {
  closeLabel: "Close LAN",
  panelTitle: "LAN",
  viewSystem: "System",
  viewLibrary: "Library",
  viewLibraryDetail: "Library / Detail",
  viewServers: "Servers",
  viewServerSettings: "Servers / Settings",
  issueStorageTitle: "Storage is not initialized",
  issueStorageDetail: "The database or migrations are still missing, so later AI diagnosis would lack full instance and runtime history.",
  issuePollingTitle: "Auto refresh is paused",
  issuePollingDetail: "Runtime polling was paused after repeated failures, so logs and health data may already be stale.",
  issueRuntimeErrorTitle: "The selected instance has an error signal",
  issueRuntimeWarningTitle: "The selected instance has a warning signal",
  issueRuntimeErrorDetail: (summary) => `Runtime health already reports an error state: ${summary}`,
  issueRuntimeWarningDetail: (summary) => `Recent logs already contain a warning pattern: ${summary}`,
  issueSteamCmdTitle: "SteamCMD is not ready",
  issueSteamCmdDetail: "The install/update pipeline is still blocked, so module setup work cannot complete cleanly.",
  issueJobsTitle: (count) => `${count} background task${count === 1 ? "" : "s"} still running`,
  issueJobsDetail: "Install, validation, or runtime work is still active, so the runtime output and job status deserve priority.",
  promptLogsLabel: "Diagnose current logs",
  promptLogsPreview: "Why did this server fail to start, hang, or exit",
  promptLogsBody: (instanceName, summary) =>
    `Use the context below to diagnose why the instance "${instanceName}" is in its current runtime state. Start with the conclusion, then evidence, then the shortest validation path. Current health summary: ${summary}`,
  promptNetworkLabel: "Check why friends cannot connect",
  promptNetworkPreview: "Check bind IP, ports, and why friends cannot connect",
  promptNetworkBody: (instanceName) =>
    `Use the context below to explain why players may fail to connect to the instance "${instanceName}". Focus on bind IP, ports, instance status, overlay hints, and startup signals. Rank the likely causes from most likely to least likely.`,
  promptSettingsLabel: "Review current settings",
  promptSettingsPreview: "Review settings_json and schema risk points",
  promptSettingsBody: (instanceName) =>
    `Use the context below to review the current configuration for "${instanceName}". Call out fields that are likely to cause startup failure, connectivity issues, or unnecessary maintenance cost, and suggest safer defaults.`,
  promptModuleLabel: "Design a default server profile",
  promptModulePreview: "Design a default server profile for a small team",
  promptModuleBody: (moduleName) =>
    `Use the module context below to design a solid default server profile for "${moduleName}" that fits LAN or small-team play. Explain why the key defaults should be chosen that way.`,
  promptInstallLabel: "Explain the install chain",
  promptInstallPreview: "Explain SteamCMD, the install path, and prerequisites",
  promptInstallBody: (moduleName) =>
    `Use the module and install context below to explain the install chain for "${moduleName}", the common failure points, and the safest step-by-step checks.`,
  promptSystemLabel: "Summarize current system risks",
  promptSystemPreview: "Summarize the host and instance issues to handle first",
  promptSystemBody:
    "Summarize the top three issues worth handling first in the system and project context below. Rank them by impact and explain why each item matters.",
  promptOverviewLabel: "Analyze current status",
  promptOverviewPreview: "Surface the important details and warning signals on this page",
  promptOverviewBody:
    "Use the current page context below to summarize the overall status, key facts, and warning signals. Start with a concise conclusion, then explain what deserves attention.",
  promptOptimizationLabel: "Recommend improvements",
  promptOptimizationPreview: "Improve stability, performance, and maintenance cost",
  promptOptimizationBody:
    "Use the host and project context below to recommend improvements for stability, performance, and maintenance cost. Rank the suggestions by expected return.",
  promptNextStepsLabel: "Plan next steps",
  promptNextStepsPreview: "Turn the current context into prioritized actions",
  promptNextStepsBody:
    "Use the current context below to plan the next actions in priority order. State the goal and completion criteria for each step.",
  exportInstructions: [
    "Please reason only from the supplied context and do not assume missing facts.",
    "Answer with conclusion first, then evidence, then next steps.",
    "If there is risk, call out the most likely root cause and the cheapest validation step."
  ],
  actionLabels: {
    "ensure-storage": "Initialize storage",
    "ensure-steamcmd": "Prepare SteamCMD",
    "resume-runtime-refresh": "Resume auto refresh",
    "view-system": "Open system",
    "open-ai-settings": "Open BYOK settings",
    "view-library": "Open library",
    "view-servers": "Open servers",
    "refresh-launch-preview": "Refresh launch preview"
  },
  aiFieldLabels: {
    model: "model",
    baseUrl: "base URL",
    apiKey: "API key"
  }
};

function copyFor(locale: string): AssistantCopy {
  return locale === "en-US" ? enUs : zhCn;
}

function viewLabel(copy: AssistantCopy, activeView: ViewKey, serverSection: ServerWorkspaceSection, libraryPage: LibraryPageMode) {
  if (activeView === "system") {
    return copy.viewSystem;
  }
  if (activeView === "library") {
    return libraryPage === "detail" ? copy.viewLibraryDetail : copy.viewLibrary;
  }
  return serverSection === "settings" ? copy.viewServerSettings : copy.viewServers;
}

function pushIssue(issues: AssistantIssue[], issue: AssistantIssue) {
  if (issues.some((existing) => existing.id === issue.id)) {
    return;
  }
  issues.push(issue);
}

function lastLines(lines: string[], count: number) {
  return lines.slice(Math.max(lines.length - count, 0));
}

function schemaFieldSummary(moduleDetails: ModuleDetails | null) {
  if (!moduleDetails?.schema_json) {
    return null;
  }

  try {
    const parsed = JSON.parse(moduleDetails.schema_json) as { properties?: Record<string, unknown> };
    const fields = Object.keys(parsed.properties ?? {});
    if (!fields.length) {
      return null;
    }

    const preview = fields.slice(0, 18).join(", ");
    return `${fields.length} fields: ${preview}${fields.length > 18 ? ", ..." : ""}`;
  } catch {
    return null;
  }
}

function instanceRoster(bootstrap: BootstrapResponse) {
  return bootstrap.state.instances
    .slice(0, 8)
    .map((instance) => `- ${instance.name} (${instance.id}) | ${instance.module_id} | ${instance.status} | bind ${instance.bind_ip}`)
    .join("\n");
}

function moduleContextLines(moduleDetails: ModuleDetails | null) {
  if (!moduleDetails) {
    return [];
  }

  const lines = [
    `Module: ${moduleDetails.summary.name} (${moduleDetails.summary.id})`,
    `Module Version: ${moduleDetails.summary.version}`,
    `Install State: ${moduleDetails.summary.install_state}`,
    `Steam App ID: ${moduleDetails.summary.steam_app_id ?? "-"}`,
    `Default Ports: ${
      moduleDetails.default_ports.length
        ? moduleDetails.default_ports.map((port) => `${port.name}/${port.protocol}:${port.port}`).join(", ")
        : "-"
    }`
  ];

  if (moduleDetails.install?.shared_game_dir) {
    lines.push(`Install Directory: ${moduleDetails.install.shared_game_dir}`);
  }
  if (moduleDetails.process?.executable) {
    lines.push(`Executable: ${moduleDetails.process.executable}`);
  }
  if (moduleDetails.process?.window_policy) {
    lines.push(`Window Policy: ${moduleDetails.process.window_policy}`);
  }
  if (moduleDetails.process?.host_notes) {
    lines.push(`Host Notes: ${moduleDetails.process.host_notes}`);
  }

  const schemaSummary = schemaFieldSummary(moduleDetails);
  if (schemaSummary) {
    lines.push(`Schema: ${schemaSummary}`);
  }

  return lines;
}

function buildContextPayload(copy: AssistantCopy, input: AssistantBuildInput, t?: TranslateFn) {
  const aiStatus = getAiSettingsStatus(input.aiSettings);
  const selectedLogLines =
    input.selectedLogDocument?.lines.length
      ? input.selectedLogDocument.lines
      : input.selectedRuntime?.log_tail.lines ?? [];
  const relevantModule =
    input.activeView === "library" ? input.selectedModuleDetails : input.selectedInstanceModuleDetails;
  const contextLines = [
    "# LanGame Assistant Context",
    `Generated At: ${new Date().toISOString()}`,
    `Active View: ${viewLabel(copy, input.activeView, input.serverWorkspaceSection, input.libraryPage)}`,
    `Storage Ready: ${input.storageReady}`,
    ...(input.storageReady
      ? [
          `Schema Version: ${input.bootstrap.state.storage.schema_version ?? 0}`,
          `Module Count: ${input.bootstrap.state.modules.length}`,
          `Instance Count: ${input.bootstrap.state.instances.length}`,
          `Running Instances: ${input.bootstrap.state.snapshot.running_instances ?? 0}`,
          `Active Jobs: ${input.activeJobsCount}`,
          `Overlay Families: ${input.overlayNames.length ? input.overlayNames.join(", ") : "-"}`,
          `SteamCMD Ready: ${input.steamCmdStatus?.ready ?? false}`,
          `Runtime Refresh Paused: ${input.runtimeAutoRefreshPaused}`,
          `Last Refresh Error: ${input.runtimeRefreshIssue?.message ?? "-"}`
        ]
      : []),
    `AI Enabled: ${input.aiSettings.enabled}`,
    `AI Provider: ${aiStatus.providerLabel}`,
    `AI Deployment: ${aiStatus.deployment}`,
    `AI Model: ${input.aiSettings.model || "-"}`,
    `AI Base URL: ${input.aiSettings.baseUrl || "-"}`,
    `AI Ready: ${aiStatus.ready}`,
    ""
  ];

  if (!input.storageReady) {
    return contextLines.join("\n").trim();
  }

  if (input.bootstrap.state.instances.length) {
    contextLines.push("## Instance Roster");
    contextLines.push(instanceRoster(input.bootstrap));
    contextLines.push("");
  }

  if (input.selectedInstanceDetails) {
    contextLines.push("## Selected Instance");
    contextLines.push(`Name: ${input.selectedInstanceDetails.summary.name}`);
    contextLines.push(`ID: ${input.selectedInstanceDetails.summary.id}`);
    contextLines.push(`Module: ${input.selectedInstanceDetails.summary.module_id}`);
    contextLines.push(`Status: ${input.selectedInstanceDetails.summary.status}`);
    contextLines.push(`Bind IP: ${input.selectedInstanceDetails.summary.bind_ip}`);
    contextLines.push(`Autostart: ${input.selectedInstanceDetails.summary.autostart}`);
    contextLines.push(`Config File: ${input.selectedInstanceDetails.config_file_path}`);
    contextLines.push(
      `Ports: ${
        input.selectedInstanceDetails.ports.length
          ? input.selectedInstanceDetails.ports.map((port) => `${port.name}/${port.protocol}:${port.port}`).join(", ")
          : "-"
      }`
    );
    contextLines.push("");
    contextLines.push("## Settings JSON");
    contextLines.push(input.selectedInstanceDetails.settings_json || "{}");
    contextLines.push("");
  }

  const moduleLines = moduleContextLines(relevantModule);
  if (moduleLines.length) {
    contextLines.push("## Module Context");
    contextLines.push(...moduleLines);
    contextLines.push("");
  }

  if (input.selectedRuntime) {
    const latestRun = input.selectedRuntime.recent_runs[0] ?? null;
    contextLines.push("## Runtime Health");
    contextLines.push(`Status: ${input.selectedRuntime.health.status ?? "-"}`);
    contextLines.push(`Summary: ${input.selectedRuntime.health.summary}`);
    contextLines.push(`Matched Line: ${input.selectedRuntime.health.matched_line ?? "-"}`);
    contextLines.push(`Latest Run Status: ${latestRun?.status ?? "-"}`);
    contextLines.push(`Latest Exit Code: ${latestRun?.exit_code ?? "-"}`);
    contextLines.push(`Latest Log Path: ${input.selectedRuntime.log_tail.source_path ?? "-"}`);
    contextLines.push("");
  }

  const launchPlan = selectedAssistantLaunchPlan(input);
  const launchIssues = buildAssistantLaunchIssues(input, t);
  if (launchPlan || launchIssues.length) {
    contextLines.push("## Launch Preview");
    if (launchPlan) {
      const ignoresPortPreflight = assistantIgnoresActivePortPreflight(input);
      const hasBlockingIssue = launchIssues.some((issue) => issue.severity === "critical");
      contextLines.push(`Executable Path: ${launchPlan.executable_path}`);
      contextLines.push(`Executable Exists: ${launchPlan.executable_exists}`);
      contextLines.push(`Ready to Launch: ${ignoresPortPreflight && !hasBlockingIssue
        ? "not evaluated while instance is active"
        : launchPlan.ready_to_launch !== false && !hasBlockingIssue}`);
      if (ignoresPortPreflight) {
        contextLines.push("Port Preflight: omitted because the selected instance is already active. The startup bind probe cannot distinguish its own ports from external conflicts.");
      }
      contextLines.push(`Working Directory: ${launchPlan.working_directory}`);
      contextLines.push(`Install Root: ${launchPlan.install_root}`);
      contextLines.push(`Command Line: ${launchPlan.command_line}`);
    }
    contextLines.push(...launchIssues.map((issue) => `[${issue.severity}] ${issue.title}: ${issue.detail}`));
    contextLines.push("");
  }

  if (selectedLogLines.length) {
    contextLines.push("## Recent Log Excerpt");
    contextLines.push(lastLines(selectedLogLines, 60).join("\n"));
    contextLines.push("");
  }

  return contextLines.join("\n").trim();
}

function buildPromptPayload(prompt: string, contextPayload: string, copy: AssistantCopy) {
  return [
    "Prompt:",
    prompt,
    "",
    "Instructions:",
    ...copy.exportInstructions.map((line) => `- ${line}`),
    "",
    "Context:",
    contextPayload
  ].join("\n");
}

function createPromptCard(
  id: string,
  label: string,
  preview: string,
  prompt: string,
  contextPayload: string,
  copy: AssistantCopy
): AssistantPromptCard {
  return {
    id,
    label,
    preview,
    prompt,
    payload: buildPromptPayload(prompt, contextPayload, copy)
  };
}

function buildPrompts(copy: AssistantCopy, input: AssistantBuildInput, contextPayload: string): AssistantPromptCards {
  const runtimeSummary = input.selectedRuntime?.health.summary
    ?? input.selectedLaunchPlanError
    ?? selectLocaleText(input.locale, "暂无运行状态摘要。", "No runtime health summary yet.");
  const systemPrompt = createPromptCard(
    "system-summary",
    copy.promptSystemLabel,
    copy.promptSystemPreview,
    copy.promptSystemBody,
    contextPayload,
    copy
  );

  if (input.selectedInstanceDetails) {
    const instanceName = input.selectedInstanceDetails.summary.name;
    const logsPrompt = copy.promptLogsBody(instanceName, runtimeSummary);
    const networkPrompt = copy.promptNetworkBody(instanceName);
    const settingsPrompt = copy.promptSettingsBody(instanceName);
    return [
      createPromptCard("logs", copy.promptLogsLabel, copy.promptLogsPreview, logsPrompt, contextPayload, copy),
      createPromptCard("network", copy.promptNetworkLabel, copy.promptNetworkPreview, networkPrompt, contextPayload, copy),
      createPromptCard("settings", copy.promptSettingsLabel, copy.promptSettingsPreview, settingsPrompt, contextPayload, copy),
      systemPrompt
    ];
  }

  if (input.selectedModuleDetails) {
    const moduleName = input.selectedModuleDetails.summary.name;
    const modulePrompt = copy.promptModuleBody(moduleName);
    const installPrompt = copy.promptInstallBody(moduleName);
    return [
      createPromptCard(
        "module-defaults",
        copy.promptModuleLabel,
        copy.promptModulePreview,
        modulePrompt,
        contextPayload,
        copy
      ),
      createPromptCard(
        "install-chain",
        copy.promptInstallLabel,
        copy.promptInstallPreview,
        installPrompt,
        contextPayload,
        copy
      ),
      systemPrompt,
      createPromptCard(
        "next-steps",
        copy.promptNextStepsLabel,
        copy.promptNextStepsPreview,
        copy.promptNextStepsBody,
        contextPayload,
        copy
      )
    ];
  }

  return [
    createPromptCard(
      "page-overview",
      copy.promptOverviewLabel,
      copy.promptOverviewPreview,
      copy.promptOverviewBody,
      contextPayload,
      copy
    ),
    systemPrompt,
    createPromptCard(
      "optimization",
      copy.promptOptimizationLabel,
      copy.promptOptimizationPreview,
      copy.promptOptimizationBody,
      contextPayload,
      copy
    ),
    createPromptCard(
      "next-steps",
      copy.promptNextStepsLabel,
      copy.promptNextStepsPreview,
      copy.promptNextStepsBody,
      contextPayload,
      copy
    )
  ];
}

export function buildAssistantViewModel(
  input: AssistantBuildInput,
  t?: TranslateFn
): AssistantViewModel {
  if (input.selectedRuntime) {
    input = {
      ...input,
      selectedRuntime: {
        ...input.selectedRuntime,
        health: {
          ...input.selectedRuntime.health,
          summary: localizeRuntimeHealthSummary(input.selectedRuntime.health, input.locale, t)
        }
      }
    };
  }
  const copy = copyFor(input.locale);
  const contextPayload = buildContextPayload(copy, input, t);

  if (!input.storageReady) {
    return {
      closeLabel: copy.closeLabel,
      issues: [],
      panelTitle: copy.panelTitle,
      prompts: [],
      contextPayload
    };
  }

  const issues: AssistantIssue[] = [];
  const healthStatus = String(input.selectedRuntime?.health.status ?? "").toLowerCase();

  if (input.runtimeAutoRefreshPaused) {
    pushIssue(issues, {
      id: "runtime-refresh-paused",
      severity: "warning",
      title: copy.issuePollingTitle,
      detail: copy.issuePollingDetail,
      action: {
        id: "resume-runtime-refresh",
        label: copy.actionLabels["resume-runtime-refresh"]
      }
    });
  }

  if (healthStatus === "error") {
    pushIssue(issues, {
      id: "runtime-error",
      severity: "critical",
      title: copy.issueRuntimeErrorTitle,
      detail: copy.issueRuntimeErrorDetail(input.selectedRuntime?.health.summary ?? "-"),
      action: {
        id: "view-servers",
        label: copy.actionLabels["view-servers"]
      }
    });
  } else if (healthStatus === "warning") {
    pushIssue(issues, {
      id: "runtime-warning",
      severity: "warning",
      title: copy.issueRuntimeWarningTitle,
      detail: copy.issueRuntimeWarningDetail(input.selectedRuntime?.health.summary ?? "-"),
      action: {
        id: "view-servers",
        label: copy.actionLabels["view-servers"]
      }
    });
  }

  issues.push(...buildAssistantLaunchIssues(input, t));

  if (input.activeView === "library" && input.steamCmdStatus && !input.steamCmdStatus.ready) {
    pushIssue(issues, {
      id: "steamcmd-missing",
      severity: "warning",
      title: copy.issueSteamCmdTitle,
      detail: copy.issueSteamCmdDetail,
      action: {
        id: "ensure-steamcmd",
        label: copy.actionLabels["ensure-steamcmd"]
      }
    });
  }

  if (input.activeJobsCount > 0) {
    pushIssue(issues, {
      id: "background-jobs",
      severity: "info",
      title: copy.issueJobsTitle(input.activeJobsCount),
      detail: copy.issueJobsDetail,
      action: {
        id: "view-servers",
        label: copy.actionLabels["view-servers"]
      }
    });
  }

  const prompts = buildPrompts(copy, input, contextPayload);
  const severityOrder: AssistantSeverity[] = ["critical", "warning", "info"];
  issues.sort((left, right) => severityOrder.indexOf(left.severity) - severityOrder.indexOf(right.severity));

  return {
    closeLabel: copy.closeLabel,
    issues,
    panelTitle: copy.panelTitle,
    prompts,
    contextPayload
  };
}
