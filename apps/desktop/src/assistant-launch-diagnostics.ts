import type { AssistantBuildInput, AssistantIssue, AssistantSeverity } from "./assistant-types";
import type { LaunchPlan } from "./types";
import type { TranslateFn } from "./i18n";
import { formatDesktopError } from "./desktop-error-message";
import { formatLaunchValidationIssue } from "./launch-validation-message";

type LaunchValidationIssue = NonNullable<LaunchPlan["validation_issues"]>[number];
type LaunchInput = Pick<AssistantBuildInput,
  "locale" | "storageReady" | "selectedInstanceId" | "selectedInstanceDetails" | "bootstrap"
  | "selectedLaunchPlan" | "selectedLaunchPlanError"
>;

const issueTitles: Record<string, [string, string]> = {
  install_root_missing: ["服务端安装目录缺失", "Server installation directory is missing"],
  config_dir_missing: ["实例配置目录缺失", "Instance configuration directory is missing"],
  working_directory_missing: ["启动工作目录缺失", "Launch working directory is missing"],
  launch_working_directory_incompatible: ["启动工作目录不受支持", "Launch working directory is unsupported"],
  launch_executable_missing: ["启动可执行文件缺失", "Launch executable is missing"],
  launch_preparation_required: ["启动时准备实例程序", "Instance executable prepared on launch"],
  launch_required_file_missing: ["启动所需文件缺失", "Required launch file is missing"],
  port_binding_unavailable: ["启动端口不可用", "Launch port is unavailable"],
  bind_ip_invalid: ["绑定地址无效", "Bind address is invalid"],
  managed_launch_option_conflict: ["启动参数重复", "Launch options conflict"],
  unresolved_launch_args: ["启动参数尚未解析完整", "Launch arguments contain unresolved values"]
};

export function selectedAssistantLaunchPlan(input: LaunchInput): LaunchPlan | null {
  const plan = input.selectedLaunchPlan;
  if (!input.selectedInstanceId || input.selectedLaunchPlanError || !plan) {
    return null;
  }
  return plan.instance_id === input.selectedInstanceId ? plan : null;
}

export function assistantIgnoresActivePortPreflight(input: LaunchInput): boolean {
  const details = input.selectedInstanceDetails;
  const selected = details && details.summary.id === input.selectedInstanceId
    ? details.summary
    : input.bootstrap.state.instances.find((instance) => instance.id === input.selectedInstanceId);
  // Runtime health has no instance ID. Only keyed instance state can establish ownership here.
  return selected?.status?.toLowerCase() === "running"
    && Boolean(selectedAssistantLaunchPlan(input)?.validation_issues.some((issue) => issue.code === "port_binding_unavailable"));
}

function normalizePath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith("//") ? normalized.toLowerCase() : normalized;
}

function containsPath(root: string, path: string): boolean {
  const parent = normalizePath(root);
  const child = normalizePath(path);
  // A missing path cannot be canonicalized here; do not hide a potentially external relative target.
  if (parent.split("/").includes("..") || child.split("/").includes("..")) {
    return false;
  }
  return Boolean(parent) && (child === parent || child.startsWith(`${parent}/`));
}

function affectedPath(issue: LaunchValidationIssue, plan: LaunchPlan): string | null {
  if (issue.path) {
    return issue.path;
  }
  switch (issue.code) {
    case "install_root_missing": return plan.install_root;
    case "working_directory_missing":
    case "launch_working_directory_incompatible": return plan.working_directory;
    case "launch_executable_missing": return plan.executable_path;
    default: return null;
  }
}

function severityFor(issue: LaunchValidationIssue): AssistantSeverity {
  switch (issue.severity.toLowerCase()) {
    case "error":
    case "critical": return "critical";
    case "info": return "info";
    default: return "warning";
  }
}

function diagnosticAction(code: string, english: boolean): AssistantIssue["action"] {
  if (["install_root_missing", "launch_executable_missing", "launch_required_file_missing"].includes(code)) {
    return { id: "view-library", label: english ? "Open library" : "打开游戏库" };
  }
  return { id: "view-servers", label: english ? "Open servers" : "打开服务器页" };
}

export function buildAssistantLaunchIssues(input: LaunchInput, t?: TranslateFn): AssistantIssue[] {
  if (!input.storageReady || !input.selectedInstanceId) {
    return [];
  }
  const english = input.locale === "en-US";
  const refreshAction = { id: "refresh-launch-preview" as const, label: english ? "Refresh launch preview" : "刷新启动预览" };
  if (input.selectedLaunchPlanError) {
    return [{
      id: "launch-preview-error",
      severity: "warning",
      title: english ? "Launch preview failed" : "启动预览生成失败",
      detail: t ? formatDesktopError(t, input.selectedLaunchPlanError) : input.selectedLaunchPlanError,
      action: refreshAction
    }];
  }

  const plan = selectedAssistantLaunchPlan(input);
  if (!plan) {
    return [];
  }
  const checks = [...plan.validation_issues];
  const ignoreActivePortPreflight = assistantIgnoresActivePortPreflight(input);
  if (!plan.executable_exists && !checks.some((issue) =>
    issue.code === "launch_executable_missing" || issue.code === "launch_preparation_required"
  )) {
    checks.push({ code: "launch_executable_missing", severity: "error", message: "", path: plan.executable_path });
  }
  const missingRoots = checks
    .filter((issue) => issue.code === "install_root_missing")
    .map((issue) => affectedPath(issue, plan))
    .filter((path): path is string => Boolean(path));
  const seen = new Set<string>();
  const issues: AssistantIssue[] = [];
  for (const check of checks) {
    if (ignoreActivePortPreflight && check.code === "port_binding_unavailable") {
      continue;
    }
    const path = affectedPath(check, plan);
    // Report the missing installation once; retain missing files outside that root.
    if (["config_dir_missing", "working_directory_missing", "launch_executable_missing", "launch_required_file_missing"].includes(check.code) && path
      && missingRoots.some((root) => containsPath(root, path))) {
      continue;
    }
    const key = JSON.stringify([check.code, path ? normalizePath(path) : check.message.trim()]);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    const severity = severityFor(check);
    const title = issueTitles[check.code]?.[english ? 1 : 0]
      ?? (english ? "Launch validation reported an issue" : "启动检查发现问题");
    issues.push({
      id: `launch-validation:${key}`,
      severity,
      title,
      detail: formatLaunchValidationIssue({ ...check, path }, t, input.locale),
      action: diagnosticAction(check.code, english)
    });
  }
  if (plan.ready_to_launch === false && !ignoreActivePortPreflight && !issues.some((issue) => issue.severity === "critical")) {
    issues.push({
      id: "launch-not-ready",
      severity: "critical",
      title: english ? "Launch checks have not passed" : "启动检查未通过",
      detail: english
        ? "The server cannot start yet. Refresh the launch preview to retrieve the blocking reason."
        : "服务端当前无法启动。请刷新启动预览，查看阻止启动的具体原因。",
      action: refreshAction
    });
  }
  return issues;
}
