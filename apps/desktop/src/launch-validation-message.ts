import { translate, type TranslateFn } from "./i18n";
import type { LaunchPlan } from "./types";

export type LaunchValidationIssue = NonNullable<LaunchPlan["validation_issues"]>[number];

export function formatLaunchValidationIssue(issue: LaunchValidationIssue, t?: TranslateFn, locale = "en-US"): string {
  const path = issue.path ?? "";
  const context = issue.context ?? {};
  function render(messageKey: string, zh: string, en: string, params: Record<string, string> = {}): string {
    const fallback = locale === "en-US" ? en : zh;
    return t ? t(messageKey, params, fallback) : translate(locale === "en-US" ? "en-US" : "zh-CN", messageKey, params, fallback);
  }
  switch (issue.code) {
    case "install_root_missing":
      return render("launch.validation.installRootMissing", "服务端安装目录不存在：{path}。请在游戏库安装或修复服务端文件。", "Server installation directory does not exist: {path}. Install or repair the server files in the library.", { path });
    case "config_dir_missing":
      return render("launch.validation.configDirMissing", "实例配置目录不存在：{path}。请保存实例配置后再启动。", "Instance configuration directory does not exist: {path}. Save the instance configuration before launch.", { path });
    case "working_directory_missing":
      return render("launch.validation.workingDirectoryMissing", "启动工作目录不存在：{path}。请检查服务端安装路径。", "Launch working directory does not exist: {path}. Check the server installation path.", { path });
    case "launch_working_directory_incompatible":
      return render("launch.validation.workingDirectoryIncompatible", "启动程序无法使用此工作目录：{path}。请使用较短的本地路径，避免特殊目录名。", "The launch program cannot use this working directory: {path}. Use a shorter local path without special directory names.", { path });
    case "launch_executable_missing":
      return render("launch.validation.executableMissing", "启动程序不存在：{path}。请在游戏库安装或修复服务端文件。", "Launch executable does not exist: {path}. Install or repair the server files in the library.", { path });
    case "launch_preparation_required":
      return render("launch.validation.preparationRequired", "服务端源文件已就绪，启动时会自动为此实例准备启动程序。", "Server source files are ready. The launch executable will be prepared automatically for this instance when starting.");
    case "launch_required_file_missing":
      return path
        ? render("launch.validation.requiredFileMissing", "启动所需文件不存在：{path}。请检查启动文件路径，或修复服务端文件。", "Required launch file does not exist: {path}. Check the launch file path or repair the server files.", { path })
        : render("launch.validation.requiredArgumentMissing", "启动参数 {argument} 缺少所需文件路径。请补全后再启动。", "Launch argument {argument} is missing its required file path. Provide the path before starting.", { argument: context.argument ?? "-jar" });
    case "port_binding_unavailable":
      return render("launch.validation.portUnavailable", "启动端口无法绑定：{port_name}（{protocol}/{address}）。请检查占用进程、绑定地址和端口配置。", "Launch port cannot be bound: {port_name} ({protocol}/{address}). Check conflicting processes, the bind address, and the port configuration.", {
        port_name: context.port_name ?? "", protocol: context.protocol ?? "", address: context.address ?? ""
      });
    case "bind_ip_invalid":
      return render("launch.validation.bindIpInvalid", "绑定地址无效：{bind_ip}。请检查实例的网络配置。", "Bind address is invalid: {bind_ip}. Check the instance network configuration.", { bind_ip: context.bind_ip ?? "" });
    case "managed_launch_option_conflict":
      return render("launch.validation.optionsConflict", "{field} 中的自定义启动参数与实例配置重复。请移除重复参数。", "Custom launch arguments in {field} repeat options managed by instance settings. Remove the duplicate options.", { field: context.field ?? "" });
    case "unresolved_launch_args":
      return render("launch.validation.unresolvedArgs", "尚有 {count} 段启动参数未解析。请检查实例配置中的参数值。", "{count} launch argument segments contain unresolved values. Check the instance configuration.", { count: context.count ?? "?" });
    case "launch_environment_invalid":
      return render("launch.validation.environmentInvalid", "模块的进程环境配置无效或不支持当前启动方式。请检查模块配置。", "The module process environment is invalid or unsupported by this launch method. Check the module configuration.");
    default:
      return issue.message;
  }
}
