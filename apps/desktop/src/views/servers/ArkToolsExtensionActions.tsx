import { useEffect, useRef, useState } from "react";
import { arkToolsHostAvailable, readArkToolsStatus, type ArkToolsStatus } from "../../api-ark-tools";
import { describeError } from "../../app-state";
import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import { useArkToolsInstallation } from "./useArkToolsInstallation";

interface Props {
  instanceId: string; instanceName: string; status: string; settingsJson: string;
  processKey: string; runId: number | null; runtimeRunning: boolean; busy: boolean;
}

/** Extension maintenance belongs to the world toolbar, independent of the selected spawn tool. */
export function ArkToolsExtensionActions({ instanceId, instanceName, status, settingsJson, processKey, runId, runtimeRunning, busy }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const host = arkToolsHostAvailable();
  const [extension, setExtension] = useState<ArkToolsStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const installation = useArkToolsInstallation(instanceId, instanceName, host, setExtension);
  const stopped = status.toLowerCase() === "stopped" && !runtimeRunning;
  const working = busy || installation.active || installation.removing;

  useEffect(() => {
    const current = ++request.current;
    setError(null);
    if (!host || installation.active || installation.removing) {
      setChecking(false);
      return;
    }
    setExtension(null); setChecking(true);
    void readArkToolsStatus(instanceId, processKey).then(value => {
      if (request.current === current) setExtension(value);
    }, reason => {
      if (request.current === current) setError(describeError(reason));
    }).finally(() => { if (request.current === current) setChecking(false); });
    return () => { ++request.current; };
  }, [host, instanceId, processKey, runId, status, runtimeRunning, settingsJson, installation.active, installation.removing]);

  const disabled = !host || !stopped || working || checking || installation.recovering;
  const commonHelp = !host ? text("请在本机桌面客户端管理服务端扩展。", "Manage the server extension in the local desktop client.")
    : !stopped ? text("请先停止实例再安装、更新或卸载服务端扩展。", "Stop the instance before installing, updating, or uninstalling the server extension.")
    : working ? text("正在处理当前操作，请稍候。", "The current operation is in progress. Please wait.")
    : checking || installation.recovering ? text("正在检查服务端扩展，请稍候。", "Checking the server extension. Please wait.") : null;
  const updateHelp = extension?.updateAvailable ? stopped
    ? text("服务端扩展已安装，有可用更新。点击“检查并更新”完成更新。", "The server extension is installed and an update is available. Select Update to install the current version.")
    : text("服务端扩展已安装，有可用更新。停止实例后点击“检查并更新”。", "The server extension is installed and an update is available. Stop the instance, then select Update.") : null;
  const installHelp = commonHelp ?? updateHelp ?? (extension?.installed
    ? text("检查并更新本实例的服务端扩展；已是当前版本时无需重新安装。", "Check and update this instance's server extension. The current version does not need to be reinstalled.")
    : text("安装本实例的玩家地图与生物工具扩展。", "Install the player-map and creature-tools extension for this instance."));
  const removalHelp = commonHelp ?? (extension?.canUninstall
    ? text("卸载本实例的玩家地图与生物工具扩展。", "Uninstall the player-map and creature-tools extension for this instance.")
    : text("当前没有可卸载的服务端扩展。", "No managed server extension is available to uninstall."));

  return <>
    <ConfigurationActionHelp reason={installHelp}>
      <button type="button" className="secondary-button ark-spawn-tool" data-ark-tools-action="install"
        aria-label={extension?.installed ? text("检查并更新服务端扩展", "Check and update server extension") : text("安装服务端扩展", "Install server extension")} disabled={disabled}
        onClick={() => { if (!disabled) { setError(null); installation.install(); } }}>
        <ShellIcon name={installation.active ? "loader" : "download"} /><span>{extension?.installed ? text("检查并更新", "Update") : text("安装", "Install")}</span>
      </button>
    </ConfigurationActionHelp>
    <ConfigurationActionHelp reason={removalHelp}>
      <InlineConfirmAction type="button" className="secondary-button ark-spawn-tool" data-ark-tools-action="uninstall"
        aria-label={text("卸载服务端扩展", "Uninstall server extension")}
        scopeKey={`${instanceId}:${processKey}:${runId}:${status}:${runtimeRunning}`} disabled={disabled || !extension?.canUninstall}
        confirmation={text("卸载后，玩家地图和生物工具将停用。游戏存档及已经生成的生物会保留；如果还有其他 ArkApi 插件，共享框架也会保留。确认卸载此实例的服务端扩展？",
          "Uninstalling disables the player map and creature tools. Game saves and creatures already spawned are preserved. The shared framework is retained when other ArkApi plugins are present. Uninstall this instance's server extension?")}
        confirmLabel={text("确认卸载", "Confirm uninstall")} onConfirm={async () => {
          if (disabled || !extension?.canUninstall) return;
          ++request.current; setChecking(false); setError(null);
          await installation.remove();
        }}>
        <ShellIcon name={installation.removing ? "loader" : "trash"} /><span>{text("卸载", "Uninstall")}</span>
      </InlineConfirmAction>
    </ConfigurationActionHelp>
    <div className="ark-extension-feedback">
      <ActivityNotice tone="info">{!checking && !working ? updateHelp ?? "" : ""}</ActivityNotice>
      {!installation.progress && installation.issue ? <ActivityNotice tone="warning">{text("安装进度暂时不可用：", "Installation progress is unavailable: ") + installation.issue.message}</ActivityNotice> : null}
      <ActivityNotice tone="error">{error ?? installation.removalError ?? extension?.issue ?? ""}</ActivityNotice>
      <ActivityNotice tone="success">{installation.removalResult ? installation.removalResult.frameworkRetained
        ? text("服务端扩展已卸载，存档与已生成生物已保留。其他 ArkApi 插件仍在使用共享框架，框架已保留。", "Server extension uninstalled. Saves and spawned creatures are preserved. The shared framework was retained for other ArkApi plugins.")
        : text("服务端扩展已卸载，存档与已生成生物已保留。", "Server extension uninstalled. Saves and spawned creatures are preserved.") : ""}</ActivityNotice>
    </div>
  </>;
}
