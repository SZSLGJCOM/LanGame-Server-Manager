import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ConfigurationHelp, ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import { sendInstanceRuntimeCommand } from "../../api";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { InstanceDetails, InstanceRuntimeCommandResult, ModuleDetails } from "../../types";
import { requestImmediateWorldSave, resolveWorldSaveAction, worldSaveDescription, worldSaveTitle, worldSaveResultMessage } from "./world-save-action";
import { MaintenanceToolbar } from "./MaintenanceToolbar";
import { requireWorldSaveReadiness, useWorldSaveReadiness } from "./use-world-save-readiness";
import "./immediate-world-save.css";

interface Props {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  readOnly?: boolean;
  /** Further compact controls of the saves strip (e.g. the auto-save chip). */
  children?: ReactNode;
}

interface SaveState {
  scope: { instanceId: string };
  sending: boolean;
  response: InstanceRuntimeCommandResult | null;
  error: string | null;
}

export function ImmediateWorldSave({ details, moduleDetails, readOnly, children }: Props) {
  const { locale, t } = useI18n();
  const scope = useMemo(() => ({ instanceId: details.summary.id }), [details.summary.id, details.active_run?.run_id, details.active_run?.session_id]);
  const currentScope = useRef<typeof scope | null>(scope);
  currentScope.current = scope;
  const request = useRef<typeof scope | null>(null);
  const [state, setState] = useState<SaveState | null>(null);
  const model = resolveWorldSaveAction(details, moduleDetails, locale);
  const preparationReason = useWorldSaveReadiness(details.summary, details.active_run, Boolean(model.action && !model.unavailable && !readOnly), locale);
  const unavailable = model.unavailable || preparationReason;
  const visible = state?.scope === scope ? state : null;
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  useEffect(() => {
    currentScope.current = scope;
    return () => { if (currentScope.current === scope) currentScope.current = null; };
  }, [scope]);

  async function saveWorld() {
    if (readOnly || !model?.action || unavailable || request.current) return;
    const operationScope = scope;
    request.current = operationScope;
    setState({ scope: operationScope, sending: true, response: null, error: null });
    try {
      await requireWorldSaveReadiness(details, locale);
      if (currentScope.current !== operationScope) return;
      const response = await requestImmediateWorldSave(details, moduleDetails, locale, sendInstanceRuntimeCommand);
      if (currentScope.current === operationScope) {
        setState({ scope: operationScope, sending: false, response, error: null });
      }
    } catch (error) {
      if (currentScope.current === operationScope) {
        setState({ scope: operationScope, sending: false, response: null, error: describeError(error) });
      }
    } finally {
      request.current = null;
      if (currentScope.current !== operationScope && currentScope.current) setState(null);
    }
  }

  if (!model) {
    return children ? <section className="server-workbench-surface server-maintenance-card immediate-world-save"
      aria-label={t("servers.maintenance.savesTitle")}>
      <div className="server-workbench-section-label server-maintenance-card-label">
        <ShellIcon name="database" className="server-workbench-section-icon" />
        <span>{t("servers.maintenance.savesTitle")}</span>
        <MaintenanceToolbar label={t("servers.maintenance.savesTitle")}>{children}</MaintenanceToolbar>
      </div>
    </section> : null;
  }
  const status = visible?.sending ? text("正在请求保存…", "Requesting save…")
    : visible?.response ? worldSaveResultMessage(visible.response, locale) : null;
  const title = worldSaveTitle(details.summary.module_id, locale);
  return <section className="server-workbench-surface server-maintenance-card immediate-world-save" aria-label={title}>
    <div className="server-workbench-section-label server-maintenance-card-label">
      <ShellIcon name="database" className="server-workbench-section-icon" />
      <ConfigurationHelp description={worldSaveDescription(details.summary.module_id, locale)}>{(help) =>
        <span ref={help.anchorRef} {...help.interactionProps} tabIndex={0} aria-describedby={help.descriptionId}>{title}<ShellIcon name="alert-circle" className="maintenance-help-icon" /></span>}</ConfigurationHelp>
      <MaintenanceToolbar label={title}>
        <ConfigurationActionHelp reason={readOnly ? t("servers.archives.workspace.restoreForMaintenance") : unavailable || null}>
          <button type="button" className="secondary-button maintenance-action" aria-busy={Boolean(visible?.sending) || undefined}
            disabled={readOnly || Boolean(unavailable) || Boolean(state?.sending)} onClick={() => void saveWorld()}>
            <ShellIcon name="download" className="maintenance-action__icon" aria-hidden="true" />
            {state?.sending ? text("保存中…", "Saving…") : text("保存存档", "Save game")}
          </button>
        </ConfigurationActionHelp>
        {children}
      </MaintenanceToolbar>
    </div>
    {/* A save that cannot reach part of the world must say so before it is pressed; the full scope is in the title bubble. */}
    {["rimworld", "barotrauma"].includes(details.summary.module_id) && model.action
      ? <p className="form-note">{t(`servers.worldSave.limit.${details.summary.module_id}`)}</p> : null}
    <ActivityNotice tone="info">{status ? [status, visible?.response?.response_text?.replace(/\s+/g, " ").trim()].filter(Boolean).join(" · ") : ""}</ActivityNotice>
    <ActivityNotice tone="error">{visible?.error ?? ""}</ActivityNotice>
  </section>;
}
