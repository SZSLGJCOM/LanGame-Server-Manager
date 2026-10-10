import { useCallback, useEffect, useId, useRef, useState } from "react";
import { getAiSettingsStatus, sameAiSettings, type AiSettings } from "../ai-settings";
import { assistantConnectionCheckAvailable, cancelAssistantConnectionCheck, checkAssistantConnection } from "../api-assistant-connection";
import type { AssistantConnectionCheckOutput, AssistantConnectionStage } from "../assistant-connection-types";
import { isChineseLocale, useI18n } from "../i18n";
import { ShellIcon } from "./ShellIcon";
import "./app-ai-connection.css";

interface AppAiConnectionCheckProps {
  settings: AiSettings;
  persistedSettings?: AiSettings;
  disabled?: boolean;
}

interface ActiveCheck {
  id: string;
  settings: AiSettings;
  persistedSettings: AiSettings | undefined;
  cancelled: boolean;
  settingsChanged: boolean;
  cancelFailed: boolean;
  cancellation: Promise<void> | null;
}

type CheckNotice = "cancelled" | "cancelFailed" | "requestFailed" | null;

function samePersistedSettings(left: AiSettings | undefined, right: AiSettings | undefined) {
  return left === right || Boolean(left && right && sameAiSettings(left, right));
}

const diagnostics: Record<string, [string, string]> = {
  authentication_rejected: ["服务拒绝了密钥，请检查密钥及其权限。", "The service rejected the key. Check its value and permissions."],
  endpoint_not_found: ["未找到模型接口，请检查服务地址和协议。", "The model endpoint was not found. Check the service URL and protocol."],
  rate_limited: ["服务请求受限，请稍后再试。", "The service rate limit was reached. Try again later."],
  provider_unavailable: ["模型服务暂时不可用，请稍后再试。", "The model service is unavailable. Try again later."],
  request_timeout: ["模型请求超时，请检查服务连接。", "The model request timed out. Check the service connection."],
  request_failed: ["模型请求失败，请检查网络和服务地址。", "The model request failed. Check the network and service URL."],
  invalid_response: ["服务响应不符合所选协议。", "The service response does not match the selected protocol."],
  credential_unavailable: ["无法读取已保存的密钥，请重新输入密钥。", "The stored key could not be read. Enter the key again."],
  chat_response_invalid: ["模型未返回有效的聊天回复。", "The model did not return a valid chat reply."],
  tool_not_called: ["模型未调用测试工具，请确认它支持工具调用。", "The model did not call the test tool. Check its tool support."],
  tool_call_invalid: ["模型返回的工具名称或参数不正确。", "The model returned an incorrect tool name or arguments."],
  tool_result_not_consumed: ["模型未正确使用工具返回的结果。", "The model did not correctly use the returned tool result."],
  request_cancelled: ["检测已取消。", "The check was cancelled."],
  overall_timeout: ["检测已达到时间上限。", "The check reached its time limit."],
  chat_not_ready: ["聊天检测未通过，未继续检测工具。", "The chat check did not pass, so tools were not checked."],
  tool_call_not_ready: ["工具调用未通过，未继续检测结果回传。", "The tool call did not pass, so result replay was not checked."]
};

export function AppAiConnectionCheck({ settings, persistedSettings, disabled = false }: AppAiConnectionCheckProps) {
  const { locale } = useI18n();
  const chinese = isChineseLocale(locale);
  const titleId = useId();
  const noteId = useId();
  const activeRef = useRef<ActiveCheck | null>(null);
  const mountedRef = useRef(true);
  const latestRef = useRef({ settings, persistedSettings });
  latestRef.current = { settings, persistedSettings };
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [result, setResult] = useState<AssistantConnectionCheckOutput | null>(null);
  const [notice, setNotice] = useState<CheckNotice>(null);
  const ready = getAiSettingsStatus({ ...settings, enabled: true }).ready;
  const hostAvailable = assistantConnectionCheckAvailable();
  const copy = chinese ? {
    title: "连接与工具检测", check: "检测连接", checking: "正在检测…", cancel: "取消检测", cancelling: "正在停止…",
    note: "仅发送测试消息，不读取服务器或会话数据；模型服务可能计费。", details: "检测详情",
    missing: "请先填写服务地址、模型和所需的 API Key。", empty: "尚未检测当前配置。",
    unavailableHost: "请在 LGSM 桌面端或已连接的管理界面中检测。",
    running: "正在检测聊天和工具通信，最多需要 90 秒。", stopping: "正在停止检测，等待请求结束。",
    cancelled: "检测已取消。", cancelFailed: "停止请求未获确认，检测已结束。",
    requestFailed: "检测未完成。请检查配置和网络后重试。",
    passed: "通过", failed: "未通过", skipped: "未检测",
    chat: "聊天回复", toolCall: "工具调用", toolReplay: "工具结果回传",
    allPassed: "聊天和工具通信均已通过。", chatOnly: "聊天可用，工具通信尚未通过。LAN 的操作能力依赖工具通信。",
    unavailable: "聊天连接未通过，请按检测结果检查配置。", requests: "次模型请求", elapsed: "用时"
  } : {
    title: "Connection and tools", check: "Check connection", checking: "Checking…", cancel: "Cancel check", cancelling: "Stopping…",
    note: "Sends test messages only, without server or conversation data. Model service charges may apply.", details: "Check details",
    missing: "Enter a service URL, model and the required API key first.", empty: "This configuration has not been checked.",
    unavailableHost: "Run this check in the LGSM desktop app or a connected management interface.",
    running: "Checking chat and tool communication. This can take up to 90 seconds.", stopping: "Stopping the check and waiting for requests to finish.",
    cancelled: "The check was cancelled.", cancelFailed: "The stop request was not confirmed. The check has ended.",
    requestFailed: "The check could not finish. Check the configuration and network, then retry.",
    passed: "Passed", failed: "Failed", skipped: "Not checked",
    chat: "Chat reply", toolCall: "Tool call", toolReplay: "Tool result replay",
    allPassed: "Chat and tool communication passed.", chatOnly: "Chat works, but tool communication has not passed. LAN needs tools to perform operations.",
    unavailable: "The chat connection did not pass. Review the check results and configuration.", requests: "model requests", elapsed: "Elapsed"
  };

  const requestCancellation = useCallback((run: ActiveCheck) => {
    run.cancelled = true;
    if (!run.cancellation) {
      run.cancellation = Promise.resolve().then(() => cancelAssistantConnectionCheck(run.id)).catch(() => {
        run.cancelFailed = true;
      });
    }
    if (mountedRef.current && activeRef.current === run) setCancelling(true);
    return run.cancellation;
  }, []);

  useEffect(() => {
    setResult(null);
    setNotice(null);
    const run = activeRef.current;
    if (run && (!sameAiSettings(run.settings, settings) || !samePersistedSettings(run.persistedSettings, persistedSettings))) {
      run.settingsChanged = true;
      void requestCancellation(run);
    }
  }, [settings.enabled, settings.provider, settings.baseUrl, settings.model, settings.apiKey, settings.apiKeyStored,
    persistedSettings?.enabled, persistedSettings?.provider, persistedSettings?.baseUrl, persistedSettings?.model,
    persistedSettings?.apiKey, persistedSettings?.apiKeyStored, requestCancellation]);

  useEffect(() => {
    if (!disabled) return;
    setResult(null);
    setNotice(null);
    const run = activeRef.current;
    if (run) {
      run.settingsChanged = true;
      void requestCancellation(run);
    }
  }, [disabled, requestCancellation]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const run = activeRef.current;
      if (run) void requestCancellation(run);
    };
  }, [requestCancellation]);

  async function startCheck() {
    if (activeRef.current || disabled || !ready || !hostAvailable) return;
    const run: ActiveCheck = { id: crypto.randomUUID(), settings: { ...settings }, persistedSettings,
      cancelled: false, settingsChanged: false, cancelFailed: false, cancellation: null };
    activeRef.current = run;
    setBusy(true);
    setCancelling(false);
    setResult(null);
    setNotice(null);
    try {
      const output = await checkAssistantConnection(run.settings, run.id);
      const latest = latestRef.current;
      if (mountedRef.current && activeRef.current === run && !run.cancelled &&
        sameAiSettings(run.settings, latest.settings) && samePersistedSettings(run.persistedSettings, latest.persistedSettings)) {
        if (output.cancelled) setNotice("cancelled");
        else setResult(output);
      }
    } catch {
      // Provider errors and model payloads stay behind the API boundary; never echo arbitrary responses here.
      const latest = latestRef.current;
      if (mountedRef.current && activeRef.current === run && !run.cancelled &&
        sameAiSettings(run.settings, latest.settings) && samePersistedSettings(run.persistedSettings, latest.persistedSettings)) setNotice("requestFailed");
    } finally {
      // A cancellation acknowledgement alone does not release the in-flight request or permit another check.
      if (run.cancellation) await run.cancellation;
      if (activeRef.current === run) {
        activeRef.current = null;
        if (mountedRef.current) {
          setBusy(false);
          setCancelling(false);
          if (run.cancelled && !run.settingsChanged) setNotice(run.cancelFailed ? "cancelFailed" : "cancelled");
        }
      }
    }
  }

  const allPassed = result?.chat.status === "passed" && result.toolCall.status === "passed" && result.toolReplay.status === "passed";
  const summary = result ? allPassed ? copy.allPassed : result.chat.status === "passed" ? copy.chatOnly : copy.unavailable : null;
  const noticeText = notice ? copy[notice] : summary;
  const statusText = busy ? cancelling ? copy.stopping : copy.running
    : noticeText ?? (!hostAvailable ? copy.unavailableHost : ready ? copy.empty : copy.missing);
  const failed = notice === "requestFailed" || notice === "cancelFailed" || result?.chat.status === "failed";
  const stages: Array<[string, AssistantConnectionStage]> = result ? [
    [copy.chat, result.chat], [copy.toolCall, result.toolCall], [copy.toolReplay, result.toolReplay]
  ] : [];
  const seconds = (milliseconds: number) => `${(milliseconds / 1000).toFixed(1)} s`;

  return <section className="ai-connection-check" aria-labelledby={titleId} aria-busy={busy}>
    <div className="ai-connection-heading">
      <h3 id={titleId}>{copy.title}</h3>
      <div className="ai-connection-actions">
        <button type="button" className="secondary-button app-settings-inline-button" disabled={disabled || !ready || !hostAvailable || busy}
          aria-describedby={noteId} onClick={() => void startCheck()}>
          <ShellIcon name={busy ? "loader" : "zap"} /><span>{busy ? copy.checking : copy.check}</span>
        </button>
        {busy ? <button type="button" className="secondary-button app-settings-inline-button" disabled={cancelling}
          onClick={() => { const run = activeRef.current; if (run) void requestCancellation(run); }}>
          <ShellIcon name="square" /><span>{cancelling ? copy.cancelling : copy.cancel}</span>
        </button> : null}
      </div>
    </div>
    <p className={`ai-connection-status${failed ? " is-error" : allPassed ? " is-passed" : result ? " is-warning" : ""}`}
      role={failed ? "alert" : "status"}>{statusText}</p>
    <p id={noteId} className="ai-connection-note">{copy.note}</p>
    {result ? <details className="ai-connection-details" key={result.requestId} open={!allPassed}>
      <summary tabIndex={0}>{copy.details}</summary>
      <ol className="ai-connection-stages">
        {stages.map(([label, stage]) => <li key={label} className={`is-${stage.status}`}>
          <ShellIcon name={stage.status === "passed" ? "check-circle" : stage.status === "failed" ? "alert-circle" : "minus"} />
          <div className="ai-connection-stage-body">
            <span className="ai-connection-stage-label">{label}</span>
            {stage.diagnostic ? <span className="ai-connection-stage-note">{diagnostics[stage.diagnostic]?.[chinese ? 0 : 1] ?? copy.requestFailed}</span> : null}
          </div>
          <span className="ai-connection-stage-status">{copy[stage.status === "passed" ? "passed" : stage.status === "failed" ? "failed" : "skipped"]}</span>
          {stage.status !== "skipped" ? <span className="ai-connection-latency">{seconds(stage.latencyMs)}</span> : null}
        </li>)}
      </ol>
      <p className="ai-connection-note ai-connection-summary">{copy.elapsed} {seconds(result.elapsedMs)} · {result.requestCount} {copy.requests}</p>
    </details> : null}
  </section>;
}
