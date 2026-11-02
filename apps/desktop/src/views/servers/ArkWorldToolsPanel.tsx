import { useEffect, useState, type ReactNode } from "react";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { InstanceDetails } from "../../types";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import type { ArkMapToolContext } from "./ArkWorldMapWorkbench";
import { gmToolConfigurationIssue } from "./gm-tool-execution";
import { buildGmToolCommand, type GmCommandBuildResult } from "./gm-tools";
import { ArkToolsExtensionActions } from "./ArkToolsExtensionActions";

interface Props {
  details: InstanceDetails; context: ArkMapToolContext; busy: boolean; sendingToolId: string | null;
  onSend: (toolId: string, preview: GmCommandBuildResult) => Promise<void>; result: ReactNode;
}

/** World operations stay available beside either spawn panel and do not require a player target. */
export function ArkWorldToolsPanel({ details, context, busy, sendingToolId, onSend, result }: Props) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const [time, setTime] = useState("12:00");
  useEffect(() => setTime("12:00"), [context.scopeKey, details.settings_json]);
  const timePreview = buildGmToolCommand(details.summary.module_id, "ark_set_time", { time }, t);
  const clearPreview = buildGmToolCommand(details.summary.module_id, "ark_destroy_wild_dinos", {}, t);
  const unavailable = (preview: GmCommandBuildResult) => preview.error
    || gmToolConfigurationIssue(details, preview, locale)
    || (!context.running || !context.scope ? t("servers.gmTools.startFirst", undefined, "Start the server before sending GM commands.") : null);
  const timeIssue = unavailable(timePreview), clearIssue = unavailable(clearPreview);
  const settingTime = sendingToolId === "ark_set_time", clearing = sendingToolId === "ark_destroy_wild_dinos";
  return <section className="ark-world-tools" aria-label={text("世界操作", "World actions")}>
    <form className="ark-world-tool-grid" onSubmit={event => {
      event.preventDefault(); if (!busy && !timeIssue) void onSend("ark_set_time", timePreview);
    }}>
      <label className="ark-spawn-setting" htmlFor="gm-ark_set_time-time-input">
        <span>{text("时间", "Time")}</span>
        <input id="gm-ark_set_time-time-input" type="text" inputMode="numeric" value={time} placeholder="HH:MM"
          aria-invalid={Boolean(timePreview.error)} onChange={event => setTime(event.target.value)} />
      </label>
      <ConfigurationActionHelp reason={timeIssue}>
        <button type="submit" className="secondary-button ark-spawn-tool" disabled={busy || Boolean(timeIssue)}>
          <ShellIcon name={settingTime ? "loader" : "clock"} /><span>{settingTime ? text("发送中", "Sending") : text("改时间", "Set time")}</span>
        </button>
      </ConfigurationActionHelp>
      <ConfigurationActionHelp reason={clearIssue}>
        <InlineConfirmAction className="secondary-button ark-spawn-tool ark-world-tool--danger" scopeKey={`ark-clear:${context.scopeKey}`}
          aria-label={text("清除野生龙", "Clear wild dinos")}
          disabled={busy || Boolean(clearIssue)} title={text("清除当前地图的所有野生生物", "Clear all wild creatures on the current map")}
          confirmation={text("将清除当前地图的所有野生生物，无法撤销。确认执行？", "This removes every wild creature on the current map and cannot be undone. Continue?")}
          confirmLabel={text("清除", "Clear")} onConfirm={async () => { if (!busy && !clearIssue) await onSend("ark_destroy_wild_dinos", clearPreview); }}>
          <ShellIcon name={clearing ? "loader" : "trash"} /><span>{clearing ? text("发送中", "Sending") : text("清野龙", "Clear")}</span>
        </InlineConfirmAction>
      </ConfigurationActionHelp>
      <ArkToolsExtensionActions instanceId={details.summary.id} instanceName={details.summary.name}
        status={details.summary.status} settingsJson={details.settings_json} processKey={context.scope?.processKey ?? "main"}
        runId={context.scope?.runId ?? null} runtimeRunning={context.running} busy={busy} />
    </form>
    {result ? <div className="ark-world-tools-feedback">{result}</div> : null}
  </section>;
}
