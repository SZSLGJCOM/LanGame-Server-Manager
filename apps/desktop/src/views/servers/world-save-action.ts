import { selectLocaleText } from "../../i18n-config";
import { instanceHasRunningProcess, runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import { immediateWorldSaveAction } from "../../runtime-save-capability";
import type { InstanceDetails, InstanceRuntimeCommandResult, ModuleDetails, ModulePlayerActionDetails, RuntimeCommandDispatchOptions } from "../../types";
import { DONTSTARVE_SHARDS, isDontStarveShardActive } from "../settings/modules/dontstarve-shards";

export interface WorldSaveAction {
  action: ModulePlayerActionDetails | null;
  unavailable: string | null;
}

export function worldSaveDescription(moduleId: string, locale: string): string {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  if (moduleId === "rimworld") return text("请求当前在线玩家客户端保存并上传世界进度，由游戏客户端执行。离线客户端无法保存。", "Ask active clients to save and upload their progress. The game clients perform the save. Offline clients cannot be saved.");
  if (moduleId === "barotrauma") return text("保存回合间大厅中已加载的战役检查点。进行中的回合无法手动保存；不会结束当前回合。", "Save the loaded campaign checkpoint in the lobby between rounds. Ongoing rounds cannot be saved manually; this action does not end a round.");
  if (moduleId === "scum") return text("请求游戏原生保存全局状态、待保存的坐标变化和已注册的持久化实体。游戏处理保存请求，程序不确认写盘完成。备份另行复制已落盘的数据。", "Request the native save for global state, pending transform changes and registered persistent entities. The game processes the request; disk completion is not verified. A separate backup copies data already on disk.");
  return text("请求游戏服务器将当前世界写入存档。备份另行复制已写入磁盘的数据。", "Ask the game server to write its current world to disk. A separate backup copies data already on disk.");
}

export function worldSaveTitle(moduleId: string, locale: string): string {
  return moduleId === "scum" ? selectLocaleText(locale, "保存服务器状态", "Save persistent state")
    : selectLocaleText(locale, "保存存档", "Save game");
}

export function resolveWorldSaveAction(details: InstanceDetails, module: ModuleDetails | null, locale: string): WorldSaveAction {
  const moduleId = details.summary.module_id;
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const action = module?.summary.id === moduleId
    ? immediateWorldSaveAction(module)
    : null;
  const unavailable = (message: string): WorldSaveAction => ({ action, unavailable: message });
  if (!module || module.summary.id !== moduleId) return unavailable(text("保存能力尚未就绪，请刷新实例信息后重试。", "Save capability is unavailable. Refresh the instance and try again."));
  if (!action) {
    if (moduleId === "squad") return unavailable(text("原版标准对局没有可恢复的持久世界存档；备份可保留配置、名单和回放。",
      "Standard vanilla matches have no recoverable persistent world save. Backups can preserve configuration, lists, and replays."));
    return unavailable(text("当前游戏尚未接入可用的立即保存接口。请按游戏原生保存机制保留进度；创建备份仅复制已写入磁盘的数据。",
      "This game does not have a connected immediate-save action. Follow the game's native save mechanism; backups only copy data already written to disk."));
  }
  if (!instanceHasRunningProcess(details.summary, details.active_run)
    || (action.process_key && !runtimeProcessKeyIsRunning(details.active_run, action.process_key))) {
    return unavailable(text("请先启动服务器，再保存当前世界。", "Start the server before saving the current world."));
  }
  if (moduleId === "dontstarve") {
    try {
      const settings = JSON.parse(details.settings_json) as unknown;
      if (!settings || typeof settings !== "object" || Array.isArray(settings)) throw new Error("Invalid settings");
      const values = settings as Record<string, unknown>;
      if (values.shard_layout !== undefined && values.shard_layout !== "standard" && values.shard_layout !== "island_adventures") throw new Error("Invalid shard layout");
      const missing = DONTSTARVE_SHARDS.filter((shard) => isDontStarveShardActive(values, shard)
        && !runtimeProcessKeyIsRunning(details.active_run, shard));
      if (missing.length) return unavailable(text(`请先启动全部启用分片，再保存存档：${missing.join("、")}。`,
        `Start every enabled shard before saving the world: ${missing.join(", ")}.`));
    } catch {
      return unavailable(text("无法读取分片设置，请刷新后重试。", "Unable to read shard settings. Refresh and try again."));
    }
  }
  if (["source_rcon", "websocket_rcon", "telnet", "palworld_rest", "astroneer_save"].includes(action.transport ?? "")) {
    const protocol = action.transport === "palworld_rest" ? "REST API"
      : action.transport === "astroneer_save" ? text("控制台", "console")
      : action.transport === "telnet" ? "Telnet" : action.transport === "websocket_rcon" ? "WebSocket RCON" : "RCON";
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
    const port = details.ports.find((candidate) => candidate.name.toLowerCase() === (action.port_name ?? (action.transport === "telnet" ? "telnet" : "rcon")).toLowerCase());
    if (!port || port.protocol.toLowerCase() !== "tcp" || !Number.isInteger(port.port) || port.port < 1 || port.port > 65535) {
      return unavailable(text(`请配置有效的 ${protocol} TCP 端口，保存并重启服务器。`,
        `Configure a valid ${protocol} TCP port, then save and restart the server.`));
    }
  }
  return { action, unavailable: null };
}

type SaveDispatch = (instanceId: string, command: string, processKey?: string | null,
  options?: RuntimeCommandDispatchOptions) => Promise<InstanceRuntimeCommandResult>;

export function worldSaveResultMessage(result: InstanceRuntimeCommandResult, locale: string): string {
  if (result.save_completion === "requested") return selectLocaleText(locale,
    "已发送保存请求，由游戏执行保存。", "Save request sent. The game will perform the save.");
  return result.save_completion === "confirmed"
    ? selectLocaleText(locale, "保存完成。", "Save completed.")
    : selectLocaleText(locale, "保存结果未知：未能确认本次保存请求的执行结果。请检查游戏控制台，本次未自动重发。",
      "Save outcome unknown: the result of this save request could not be determined. Check the game console; the request was not resent.");
}

export async function requestImmediateWorldSave(details: InstanceDetails, module: ModuleDetails | null,
  locale: string, dispatch: SaveDispatch): Promise<InstanceRuntimeCommandResult> {
  const model = resolveWorldSaveAction(details, module, locale);
  if (!model?.action || model.unavailable) throw new Error(model?.unavailable
    ?? selectLocaleText(locale, "当前游戏不支持保存存档。", "This game does not support saving the game."));
  const result = await dispatch(details.summary.id, model.action.command_template, model.action.process_key ?? null,
    { runtimeActionId: "save_world" });
  if (result.save_completion !== "requested" && result.save_completion !== "confirmed") {
    throw new Error(worldSaveResultMessage(result, locale));
  }
  return result;
}
