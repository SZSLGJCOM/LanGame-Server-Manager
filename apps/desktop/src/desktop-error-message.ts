import type { TranslateFn } from "./i18n";
import { formatLaunchValidationIssue } from "./launch-validation-message";
import { formatSteamCmdError } from "./steamcmd-error-message";

const PROGRAM_CREATION_ERRORS = [
  ["服务器程序尚未下载完成，请先在游戏库完成安装或校验，再创建实例；本次没有启动下载。", "errors.creationDownloadIncomplete"],
  ["本地服务器程序不完整，请先在游戏库安装或校验；已有文件已保留，本次没有启动下载。", "errors.creationProgramIncomplete"],
  ["没有可导入的本地程序库目录；归档中的实例请先恢复，或使用已验证程序创建。", "errors.creationLocalLibraryMissing"],
  ["现有实例或归档中的程序缺少完整校验清单，或程序文件已修改；已有文件已保留，本次没有启动下载。可恢复原实例，或先在游戏库安装或校验服务器程序。", "errors.creationRetainedProgramUnverified"],
  ["此游戏尚不支持共享服务器程序，请使用独立安装。", "errors.creationSharingUnsupported"]
] as const;

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function stringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

// Decode application error contracts and the known assistant request-schema mismatch. Other diagnostics stay intact.
export function formatDesktopError(t: TranslateFn, error: unknown): string {
  const original = error instanceof Error ? error.message : typeof error === "string" ? error : String(error);
  let value: unknown = error instanceof Error ? error.message : error;
  if (typeof value === "string") {
    for (const [message, key] of PROGRAM_CREATION_ERRORS) {
      if (value === message) return t(key);
      if (!value.startsWith(message)) continue;
      const diagnostic = value.slice(message.length);
      if (diagnostic.startsWith(". See app log: ")
        || diagnostic.startsWith(". ") && diagnostic.includes(" Log path: ")) {
        return `${t(key)}${diagnostic}`;
      }
    }
    if (/^invalid args `input` for command `assistant_execute_operation`: unknown field `conversationMessages`, expected one of `[A-Za-z][A-Za-z0-9_]*`(?:, `[A-Za-z][A-Za-z0-9_]*`)+$/.test(value)) {
      return t("errors.assistantBackendOutOfDate");
    }
    if (!value.trimStart().startsWith("{")) return original;
    try { value = JSON.parse(value); } catch { return original; }
  }
  if (!record(value) || typeof value.code !== "string" || typeof value.message !== "string") return original;
  const installError = formatSteamCmdError(t, value);
  if (installError !== null) return installError;
  switch (value.code) {
    case "system_telemetry_contract_missing":
      return t("errors.systemTelemetryBackendOutOfDate");
    case "mod_runtime_unverified":
      if (value.reason === "minecraft_loader") return t("errors.modMinecraftRuntimeUnverified");
      if (value.reason === "bepinex") return t("errors.modBepInExRuntimeUnverified");
      break;
    case "mod_dependencies_unverified":
      if (!stringList(value.dependencies) || value.dependencies.length === 0) break;
      return t("errors.modDependenciesUnverified", { dependencies: value.dependencies.join(", ") });
    case "mod_community_mismatch":
      if (typeof value.community !== "string" || typeof value.package !== "string") break;
      return t("errors.modCommunityMismatch", { community: value.community, package: value.package });
    case "steam_workshop_browse_invalid_response":
      return t("errors.workshopBrowseInvalidResponse");
    case "steam_workshop_browse_unrecognized_response":
      return t("errors.workshopBrowseUnrecognizedResponse");
    case "steam_workshop_details_unrecognized_response":
      return t("errors.workshopDetailsUnrecognizedResponse");
    case "steam_workshop_browse_unsupported_sort":
      if (value.browse_kind === "collection" && value.sort === "subscribers") return t("errors.workshopCollectionSortUnsupported");
      break;
    case "workshop-collection-install":
      if (typeof value.item_id !== "string" || !/^[1-9]\d*$/.test(value.item_id)
        || typeof value.reason !== "string" || !["missing", "unresolved", "wrong-game", "unsupported", "client-only", "incomplete", "cycle", "empty-collection"].includes(value.reason)) break;
      return t(`errors.workshopCollection.${value.reason}`, { id: value.item_id });
    case "steam_workshop_network_failed": {
      if (typeof value.stage !== "string" || !["browse", "item_type", "details", "collection"].includes(value.stage)
        || typeof value.reason !== "string" || !["timeout", "connection", "response", "request", "http"].includes(value.reason)
        || typeof value.origin !== "string" || !["https://steamcommunity.com", "https://steamcommunity-a.akamaihd.net", "https://api.steampowered.com", "https://api.steamchina.com"].includes(value.origin)
        || (value.reason === "http" && (typeof value.status !== "number" || !Number.isInteger(value.status) || value.status < 100 || value.status > 599))) break;
      const summary = t("errors.workshopNetworkFailed", {
        stage: t(`errors.workshopStage.${value.stage}`),
        origin: value.origin,
        reason: t(`errors.workshopReason.${value.reason === "http" && value.status === 429 ? "rateLimited" : value.reason}`, { status: typeof value.status === "number" ? value.status : "" })
      });
      if (["timeout", "connection", "request"].includes(value.reason)) {
        const community = value.origin === "https://steamcommunity.com" || value.origin === "https://steamcommunity-a.akamaihd.net";
        return `${summary}\n${t(community ? "errors.workshopCommunityConnection" : "errors.workshopApiConnection")}`;
      }
      if (value.reason === "http" && typeof value.retry_after_seconds === "number"
        && Number.isSafeInteger(value.retry_after_seconds) && value.retry_after_seconds > 0) {
        return `${summary} ${t("errors.workshopRetryAfter", { seconds: value.retry_after_seconds })}`;
      }
      return summary;
    }
    case "assistant_request_interpretation_failed":
      return t("errors.assistantRequestInterpretationFailed");
    case "instance_already_running":
      if (typeof value.instance_id !== "string") break;
      return t("errors.instanceAlreadyRunning", { instance: value.instance_id });
    case "instance_not_running":
      if (typeof value.instance_id !== "string") break;
      return t("errors.instanceNotRunning", { instance: value.instance_id });
    case "module_in_use":
      if (typeof value.module_name !== "string" || !stringList(value.instances)) break;
      return t("errors.moduleInUse", { module: value.module_name, instances: value.instances.join(", ") });
    case "install_data_protected": {
      if (typeof value.module_name !== "string" || typeof value.install_root !== "string" || !Array.isArray(value.protected)) break;
      const paths: string[] = [];
      for (const item of value.protected) {
        if (!record(item) || typeof item.source !== "string" || (item.path !== null && typeof item.path !== "string")) return original;
        paths.push(typeof item.path === "string" ? item.path : item.source);
      }
      return [t("errors.installDataProtected", { module: value.module_name, path: value.install_root, count: value.protected.length }), ...paths].join("\n");
    }
    case "install_replacement_not_empty":
      if (typeof value.module_name !== "string" || typeof value.install_root !== "string") break;
      return t("errors.installReplacementNotEmpty", { module: value.module_name, path: value.install_root });
    case "install_path_not_directory":
      if (typeof value.install_root !== "string") break;
      return t("errors.installPathNotDirectory", { path: value.install_root });
    case "launch_preflight_failed": {
      if (!Array.isArray(value.issues) || !value.issues.length) break;
      const details: string[] = [];
      for (const item of value.issues) {
        if (!record(item) || typeof item.code !== "string" || typeof item.message !== "string"
          || typeof item.severity !== "string" || (item.path != null && typeof item.path !== "string")
          || (item.context != null && (!record(item.context) || !Object.values(item.context).every((entry) => typeof entry === "string")))) return original;
        const detail = formatLaunchValidationIssue({
          code: item.code, message: item.message, severity: item.severity, path: item.path as string | null | undefined,
          context: item.context as Record<string, string> | undefined
        }, t);
        const process = typeof item.display_name === "string" ? item.display_name : "";
        details.push(process ? `${process}: ${detail}` : detail);
      }
      return `${t("errors.launchPreflightFailed")}\n${details.join("\n")}`;
    }
  }
  return original;
}
