import { useEffect, useMemo, useRef, useState } from "react";
import { sendInstanceRuntimeCommand } from "../../api";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { InstanceDetails, InstanceRuntimeCommandResult, ModuleDetails } from "../../types";
import { resolveWorldSaveAction } from "./world-save-action";
import "./immediate-world-save.css";

interface Props {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  readOnly?: boolean;
}

interface SaveState {
  instanceId: string;
  sending: boolean;
  response: InstanceRuntimeCommandResult | null;
  error: string | null;
}

export function ImmediateWorldSave({ details, moduleDetails, readOnly }: Props) {
  const { locale, t } = useI18n();
  const scope = useMemo(() => ({ instanceId: details.summary.id }), [details.summary.id]);
  const currentScope = useRef<typeof scope | null>(scope);
  currentScope.current = scope;
  const request = useRef<typeof scope | null>(null);
  const [state, setState] = useState<SaveState | null>(null);
  const model = resolveWorldSaveAction(details, moduleDetails, locale);
  const visible = state?.instanceId === scope.instanceId ? state : null;
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  useEffect(() => {
    currentScope.current = scope;
    return () => { if (currentScope.current === scope) currentScope.current = null; };
  }, [scope]);

  async function saveWorld() {
    if (readOnly || !model?.action || model.unavailable || request.current) return;
    const operationScope = scope;
    request.current = operationScope;
    setState({ instanceId: scope.instanceId, sending: true, response: null, error: null });
    try {
      const response = await sendInstanceRuntimeCommand(scope.instanceId, model.action.command_template,
        model.action.process_key ?? null, { runtimeActionId: "save_world" });
      if (currentScope.current === operationScope) {
        setState({ instanceId: scope.instanceId, sending: false, response, error: null });
      }
    } catch (error) {
      if (currentScope.current === operationScope) {
        setState({ instanceId: scope.instanceId, sending: false, response: null, error: describeError(error) });
      }
    } finally {
      request.current = null;
      if (currentScope.current !== operationScope && currentScope.current) setState(null);
    }
  }

  if (!model) return null;
  const pending = visible?.response?.write_confirmation_pending;
  const status = visible?.sending ? text("正在发送保存请求…", "Sending save request…")
    : pending ? text("保存命令已受理，但仍等待写入确认。请先核对运行日志，再决定是否重试。",
      "The save command was accepted and is awaiting write confirmation. Check the runtime log before retrying.")
    : visible?.response ? text("保存请求已发送，请核对服务器响应、运行日志或存档更新时间。",
      "Save request sent. Check the server response, runtime log, or save file modification time.") : null;
  return <section className="server-workbench-surface server-maintenance-card immediate-world-save"
    aria-label={text("立即保存世界", "Save world now")}>
    <div className="server-workbench-section-label server-maintenance-card-label">
      <ShellIcon name="database" className="server-workbench-section-icon" />
      <span>{text("立即保存世界", "Save world now")}</span>
      <button type="button" className="secondary-button" disabled={readOnly || Boolean(model.unavailable) || Boolean(state?.sending)}
        onClick={() => void saveWorld()}>{state?.sending ? text("发送中…", "Sending…") : text("立即保存", "Save now")}</button>
    </div>
    <p className="form-note">{text("请求游戏服务器将当前世界写入存档。", "Ask the game server to write the current world to its save files.")}</p>
    {readOnly ? <p className="form-note" role="status">{t("servers.archives.workspace.restoreForMaintenance")}</p>
      : model.unavailable ? <p className="form-note" role="status">{model.unavailable}</p> : null}
    {status ? <p className="form-note" role="status">{status}</p> : null}
    {visible?.error ? <p className="immediate-world-save__error" role="alert">{visible.error}</p> : null}
    {visible?.response?.response_text ? <pre className="immediate-world-save__response">{visible.response.response_text}</pre> : null}
  </section>;
}
