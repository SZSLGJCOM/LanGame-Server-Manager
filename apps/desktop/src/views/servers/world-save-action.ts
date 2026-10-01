import { selectLocaleText } from "../../i18n-config";
import { instanceHasRunningProcess, runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import type { InstanceDetails, ModuleDetails, ModulePlayerActionDetails } from "../../types";

// These modules expose an explicit immediate-save action. Metadata remains the
// source of its transport and command; never execute a shutdown command list.
const WORLD_SAVE_MODULES = new Set(["minecraft", "projectzomboid", "terraria", "palworld", "unturned"]);

export interface WorldSaveAction {
  action: ModulePlayerActionDetails | null;
  unavailable: string | null;
}

export function resolveWorldSaveAction(details: InstanceDetails, module: ModuleDetails | null, locale: string): WorldSaveAction | null {
  const moduleId = details.summary.module_id;
  if (!WORLD_SAVE_MODULES.has(moduleId)) return null;
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const action = module?.summary.id === moduleId
    ? (module.runtime.player_actions ?? []).find((entry) => entry.id === "save_world" && !entry.target_required && !entry.destructive) ?? null
    : null;
  const unavailable = (message: string): WorldSaveAction => ({ action, unavailable: message });
  if (!action) return unavailable(text("保存能力尚未就绪，请刷新实例信息后重试。", "Save capability is unavailable. Refresh the instance and try again."));
  if (!instanceHasRunningProcess(details.summary, details.active_run)
    || (action.process_key && !runtimeProcessKeyIsRunning(details.active_run, action.process_key))) {
    return unavailable(text("请先启动服务器，再保存当前世界。", "Start the server before saving the current world."));
  }
  if (action.transport === "source_rcon" || action.transport === "palworld_rest") {
    const protocol = action.transport === "palworld_rest" ? "REST API" : "RCON";
    let settings: Record<string, unknown>;
    try {
      const value: unknown = JSON.parse(details.settings_json);
      if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid settings");
      settings = value as Record<string, unknown>;
    } catch {
      return unavailable(text("无法读取实例设置，请刷新后重试。", "Unable to read instance settings. Refresh and try again."));
    }
    const enabled = action.enabled_setting_key ? settings[action.enabled_setting_key] : true;
    if (!(enabled === true || enabled === 1 || String(enabled).toLowerCase() === "true")) {
      return unavailable(text(`请在实例设置中启用 ${protocol}，保存并重启服务器。`,
        `Enable ${protocol} in instance settings, then save and restart the server.`));
    }
    const password = action.password_setting_key ? settings[action.password_setting_key] : undefined;
    if (action.password_setting_key && (typeof password !== "string" || !password.trim())) {
      return unavailable(text(`请配置 ${protocol} 管理密码，保存并重启服务器。`,
        `Configure the ${protocol} administrator password, then save and restart the server.`));
    }
    const port = details.ports.find((candidate) => candidate.name.toLowerCase() === (action.port_name ?? "rcon").toLowerCase());
    if (!port || port.protocol.toLowerCase() !== "tcp" || !Number.isInteger(port.port) || port.port < 1 || port.port > 65535) {
      return unavailable(text(`请配置有效的 ${protocol} TCP 端口，保存并重启服务器。`,
        `Configure a valid ${protocol} TCP port, then save and restart the server.`));
    }
  }
  return { action, unavailable: null };
}
