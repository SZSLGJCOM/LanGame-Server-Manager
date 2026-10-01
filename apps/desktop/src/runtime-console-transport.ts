import type { ModuleDetails, RuntimeCommandDispatchOptions } from "./types";

interface RuntimeConsoleHint {
  zhCN: string;
  en: string;
}

export interface RuntimeConsoleTransportResolution {
  available: boolean;
  options: RuntimeCommandDispatchOptions | undefined;
  hint: RuntimeConsoleHint | null;
}

type TextTransport = "stdin" | "unreal_console" | "source_rcon" | "websocket_rcon" | "telnet";
interface DeclaredTransport {
  transport?: string | null;
  port_name?: string | null;
  password_setting_key?: string | null;
  enabled_setting_key?: string | null;
}
interface ConsoleRoute extends RuntimeCommandDispatchOptions {
  transport: TextTransport;
}

const nativeOptions: ConsoleRoute = { transport: "stdin" };

function normalized(value?: string | null): string | undefined {
  return value?.trim() || undefined;
}

function routeFor(declaration: DeclaredTransport): ConsoleRoute | null {
  const transport = normalized(declaration.transport)?.toLowerCase() ?? "stdin";
  if (transport === "stdin") return { ...nativeOptions };
  if (transport === "unreal_console") return { transport };
  // REST actions and HumanitZ's read-only info exchange are not arbitrary text consoles.
  if (transport !== "source_rcon" && transport !== "websocket_rcon" && transport !== "telnet") return null;
  const defaultPasswordSettingKey = transport === "telnet"
    ? "telnet_password"
    : "rcon_password";
  return {
    transport,
    portName: normalized(declaration.port_name)?.toLowerCase() ?? (transport === "telnet" ? "telnet" : "rcon"),
    passwordSettingKey: normalized(declaration.password_setting_key) ?? defaultPasswordSettingKey,
    enabledSettingKey: normalized(declaration.enabled_setting_key)
  };
}

function uniqueRoutes(declarations: DeclaredTransport[]): ConsoleRoute[] {
  const routes = new Map<string, ConsoleRoute>();
  for (const declaration of declarations) {
    const route = routeFor(declaration);
    if (route) routes.set(JSON.stringify(route), route);
  }
  return [...routes.values()];
}

function configuredBoolean(value: unknown): boolean | undefined {
  if (typeof value === "boolean") return value;
  if (typeof value !== "string") return undefined;
  switch (value.trim().toLowerCase()) {
    case "true": case "1": case "yes": case "on": return true;
    case "false": case "0": case "no": case "off": return false;
    default: return undefined;
  }
}

function routeIssue(route: ConsoleRoute, settings: Record<string, unknown> | null): RuntimeConsoleHint | null {
  if (route.transport === "stdin" || route.transport === "unreal_console") return null;
  if (!settings) return {
    zhCN: "实例设置不是有效的 JSON 对象，无法核实远程命令通道。",
    en: "Instance settings are not a valid JSON object; the remote command channel cannot be checked."
  };
  if (route.enabledSettingKey) {
    // Match the native dispatcher's defaults: RCON needs opt-in; Telnet permits an absent flag.
    const enabled = configuredBoolean(settings[route.enabledSettingKey]) ?? route.transport === "telnet";
    if (!enabled) return {
      zhCN: `远程命令通道未启用（${route.enabledSettingKey}）。`,
      en: `The remote command channel is not enabled (${route.enabledSettingKey}).`
    };
  }
  const password = settings[route.passwordSettingKey ?? "rcon_password"];
  // The native Telnet protocol explicitly permits servers without a password.
  if (route.transport !== "telnet" && (typeof password !== "string" || !password.trim())) return {
    zhCN: `远程命令通道缺少密码设置（${route.passwordSettingKey}）。`,
    en: `The remote command channel has no password configured (${route.passwordSettingKey}).`
  };
  return null;
}

function nativeFallback(issue: RuntimeConsoleHint): RuntimeConsoleTransportResolution {
  return {
    available: true,
    options: { ...nativeOptions },
    hint: {
      zhCN: `${issue.zhCN} 已使用模块声明的原生输入通道。`,
      en: `${issue.en} Using the module's declared native input channel.`
    }
  };
}

export function resolveRuntimeConsoleTransport(
  module: ModuleDetails | null,
  settingsJson: string
): RuntimeConsoleTransportResolution {
  if (!module) return {
    available: false,
    options: undefined,
    hint: {
      zhCN: "模块信息尚未加载，暂时无法确定命令通道。",
      en: "Module information has not loaded; the command channel cannot be determined yet."
    }
  };
  const actions = module.runtime.player_actions ?? [];
  const shutdown = module.runtime.shutdown?.commands ?? [];
  const routes = uniqueRoutes([...actions, ...shutdown]);
  const ark = module.summary.id === "arksurvivalascended" || module.summary.id === "arksurvivalevolved";
  // ARK selects maps through RCON. Shutdown's stdin fallback does not verify interactive map routing.
  const nativeDeclared = routes.some(route => route.transport === "stdin")
    || (!ark && shutdown.some(command => normalized(command.fallback_transport)?.toLowerCase() === "stdin"));
  // A command-specific shutdown fallback is not a competing interactive route.
  if (!routes.length && nativeDeclared) return { available: true, options: { ...nativeOptions }, hint: null };
  if (!routes.length) return {
    available: true,
    options: { ...nativeOptions },
    hint: {
      zhCN: "该模块尚未声明通用文本命令通道；当前使用原生输入，是否接收或执行须由服务器响应确认。",
      en: "This module has no declared general text command channel. Native input is used; server receipt and execution require a server response."
    }
  };

  let settings: Record<string, unknown> | null = null;
  try {
    const parsed: unknown = JSON.parse(settingsJson);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) settings = parsed as Record<string, unknown>;
  } catch { /* Native input does not require JSON settings; remote routes report this below. */ }
  const evaluated = routes.map(route => ({ route, issue: routeIssue(route, settings) }));
  const available = evaluated.filter(candidate => !candidate.issue);
  if (available.length > 1) return {
    available: false,
    options: undefined,
    hint: {
      zhCN: "模块声明了多个可用的文本命令通道，无法确定命令接收端。",
      en: "The module declares multiple available text command channels; the command destination is ambiguous."
    }
  };
  if (available.length === 1) return { available: true, options: available[0].route, hint: null };
  const issue = evaluated[0].issue!;
  if (nativeDeclared) return nativeFallback(issue);
  return {
    available: false,
    options: undefined,
    hint: evaluated.length === 1 ? issue : {
      zhCN: "模块声明的文本命令通道均不可用，请检查相应通道的启用和密码设置。",
      en: "None of the module's declared text command channels is available. Check their enable and password settings."
    }
  };
}
