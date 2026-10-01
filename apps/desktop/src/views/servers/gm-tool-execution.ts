import { useEffect, useRef, useState } from "react";
import { selectLocaleText } from "../../i18n-config";
import type { InstanceDetails, InstanceRuntimeCommandResult, RuntimeCommandDispatchOptions } from "../../types";
import type { GmCommandBuildResult } from "./gm-tools";

export interface GmBatchResult {
  total: number;
  responses: InstanceRuntimeCommandResult[];
  error: string | null;
  stopped: boolean;
}

type GmCommandSender = (
  instanceId: string,
  command: string,
  processKey: string | null,
  options: RuntimeCommandDispatchOptions
) => Promise<InstanceRuntimeCommandResult>;

interface GmExecutionState extends GmBatchResult {
  instanceId: string;
  toolId: string;
  sending: boolean;
}

export function gmToolConfigurationIssue(details: InstanceDetails, preview: GmCommandBuildResult, locale: string): string | null {
  const options = preview.dispatchOptions;
  if (!["source_rcon", "palworld_rest"].includes(options.transport ?? "")) return null;
  const protocol = options.transport === "palworld_rest" ? "REST API" : "RCON";
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  let settings: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(details.settings_json);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("Invalid settings");
    settings = parsed as Record<string, unknown>;
  } catch {
    return text("无法读取实例设置，请刷新后重试。", "Unable to read instance settings. Refresh and try again.");
  }
  const enabled = options.enabledSettingKey ? settings[options.enabledSettingKey] : true;
  if (!(enabled === true || enabled === 1 || String(enabled).toLowerCase() === "true")) {
    return text(`请在实例设置中启用 ${protocol}，保存并重启服务器后再使用此工具。`,
      `Enable ${protocol} in instance settings, then save and restart the server before using this tool.`);
  }
  const password = options.passwordSettingKey ? settings[options.passwordSettingKey] : undefined;
  if (options.passwordSettingKey && (typeof password !== "string" || !password.trim())) {
    return text(`请在实例设置中配置 ${protocol} 管理密码，保存并重启服务器。`,
      `Configure the ${protocol} administrator password in instance settings, then save and restart the server.`);
  }
  const port = details.ports.find((candidate) => candidate.name.toLowerCase() === (options.portName ?? "rcon").toLowerCase());
  if (!port || port.protocol.toLowerCase() !== "tcp" || !Number.isInteger(port.port) || port.port < 1 || port.port > 65535) {
    return text(`请在实例设置中配置有效的 ${protocol} TCP 端口，保存并重启服务器。`,
      `Configure a valid ${protocol} TCP port in instance settings, then save and restart the server.`);
  }
  return null;
}

// Dispatch is sequential. A pending write has an unknown outcome and must not
// be followed by more mutations or an automatic retry.
export async function executeGmCommandBatch(
  instanceId: string,
  preview: GmCommandBuildResult,
  send: GmCommandSender,
  signal: AbortSignal,
  onProgress: (result: GmBatchResult) => void,
  errorText: (error: unknown) => string
): Promise<GmBatchResult> {
  let result: GmBatchResult = { total: preview.commands.length, responses: [], error: null, stopped: false };
  for (const command of preview.commands) {
    if (signal.aborted) return { ...result, stopped: true };
    try {
      const response = await send(instanceId, command, preview.processKey, preview.dispatchOptions);
      result = { ...result, responses: [...result.responses, response] };
      onProgress(result);
      if (response.write_confirmation_pending) return result;
    } catch (error) {
      return { ...result, error: errorText(error) };
    }
  }
  return result;
}

export function useGmToolExecution(instanceId: string, send: GmCommandSender, errorText: (error: unknown) => string) {
  const active = useRef<{ instanceId: string; controller: AbortController } | null>(null);
  const mounted = useRef(false);
  const [state, setState] = useState<GmExecutionState | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      active.current?.controller.abort();
    };
  }, []);
  useEffect(() => {
    // Switching instances stops the unsent part of a batch. Keep the lock until
    // the native request settles: IPC already in flight cannot be cancelled.
    if (active.current && active.current.instanceId !== instanceId) active.current.controller.abort();
  }, [instanceId]);

  async function run(toolId: string, preview: GmCommandBuildResult) {
    if (active.current) return;
    const request = { instanceId, controller: new AbortController() };
    active.current = request;
    const update = (result: GmBatchResult, sending: boolean) => {
      if (mounted.current) setState({ ...result, instanceId, toolId, sending });
    };
    update({ total: preview.commands.length, responses: [], error: null, stopped: false }, true);
    try {
      const result = await executeGmCommandBatch(instanceId, preview, send, request.controller.signal,
        (progress) => update(progress, true), errorText);
      update(result, false);
    } finally {
      active.current = null;
    }
  }

  return {
    busy: Boolean(state?.sending),
    sendingToolId: state?.sending && state.instanceId === instanceId ? state.toolId : null,
    result: state?.instanceId === instanceId ? state : null,
    run
  };
}

export function gmExecutionText(locale: string, result: GmExecutionState) {
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const delivered = result.responses.filter((response) => !response.write_confirmation_pending).length;
  const pending = result.responses.some((response) => response.write_confirmation_pending);
  if (result.error) return text(
    `已发送 ${delivered}/${result.total} 条；第 ${result.responses.length + 1} 条未获确认，其余已停止。${result.error}`,
    `Sent ${delivered}/${result.total}; command ${result.responses.length + 1} was not confirmed. Remaining commands stopped. ${result.error}`);
  if (pending) return text(
    `已发送 ${delivered}/${result.total} 条；另 1 条仍等待写入确认。剩余命令未发送，请先检查运行日志再决定是否重试。`,
    `Sent ${delivered}/${result.total}; 1 command is awaiting write confirmation. Remaining commands were not sent. Check the runtime log before retrying.`);
  if (result.stopped) return text(
    `已发送 ${delivered}/${result.total} 条；切换实例后，剩余命令未发送。`,
    `Sent ${delivered}/${result.total}; remaining commands were not sent after switching instances.`);
  if (result.sending) return text(
    `正在发送 ${delivered}/${result.total} 条命令…`, `Sending commands: ${delivered}/${result.total}…`);
  return text(
    `已发送 ${delivered}/${result.total} 条命令。请核对服务器响应、运行日志或游戏内结果。`,
    `Sent ${delivered}/${result.total} commands. Check the server response, runtime log, or in-game result.`);
}
