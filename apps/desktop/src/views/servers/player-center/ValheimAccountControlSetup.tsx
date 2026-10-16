import { useState } from "react";
import { prepareValheimPlayerControl } from "../../../api";
import { ActivityNotice } from "../../../components/ActivityNotice";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { InstanceDetails } from "../../../types";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";

interface Props {
  details: InstanceDetails;
  connected: boolean;
  readOnly?: boolean;
  locale: LocaleCode;
  onPrepare?: (instanceId: string) => Promise<InstanceDetails>;
}

/** Native account collection is explicitly installed while this instance is stopped. */
export function ValheimAccountControlSetup(props: Props) {
  const [preparing, setPreparing] = useState(false);
  const [prepared, setPrepared] = useState(false);
  if (props.connected) return null;
  const stopped = props.details.summary.status === "Stopped" && props.details.summary.active_process_count === 0;
  const reason = preparing ? selectLocaleText(props.locale, "正在准备账号管理扩展，请等待完成。", "The account management extension is being prepared. Wait for it to finish.")
    : props.readOnly ? selectLocaleText(props.locale, "当前工作区只读，无法启用账号管理。", "This workspace is read-only; account management cannot be enabled.")
      : !stopped ? selectLocaleText(props.locale, "请先正常停止服务器，再启用账号管理。", "Stop the server normally before enabling account management.")
        : prepared ? selectLocaleText(props.locale, "账号管理扩展已准备；启动服务器后可自动读取平台身份。", "The account management extension is prepared. Start the server to collect platform identities automatically.") : null;
  const disabled = Boolean(reason);
  // InlineConfirmAction owns its pending state. Changing its external disabled
  // prop for the same request would reset its error review before a retry.
  const unavailable = Boolean(props.readOnly || !stopped || prepared);
  const label = selectLocaleText(props.locale, "启用账号管理", "Enable account management");
  return <section className="player-access-selected-actions" aria-label={label}>
    <ConfigurationHelp description={reason}>{(help) => <div className="player-access-total-action" data-valheim-control-setup
      ref={help.anchorRef} {...help.interactionProps} tabIndex={disabled ? 0 : undefined}
      role={disabled ? "group" : undefined} aria-disabled={disabled || undefined}
      aria-label={disabled ? label : undefined} aria-describedby={help.descriptionId}>
      <InlineConfirmAction className="secondary-button player-center-action-button" disabled={unavailable}
        aria-describedby={help.descriptionId} scopeKey={JSON.stringify([props.details.summary.id, stopped, props.readOnly])}
        confirmation={selectLocaleText(props.locale,
          "为此实例准备独立程序目录和 BepInEx 服务端管理扩展？这会改变该实例的程序运行环境，保留已有存档和世界规则。启动后将自动采集平台身份。",
          "Prepare an independent program directory and BepInEx server management extension for this instance? This changes its program runtime environment and preserves existing saves and world rules. Platform identities will be collected automatically after startup.")}
        onConfirm={async () => {
          if (disabled) return;
          setPreparing(true);
          try {
            const result = await (props.onPrepare ?? prepareValheimPlayerControl)(props.details.summary.id);
            if (result.summary.id !== props.details.summary.id || result.summary.module_id !== "valheim") throw new Error("The account management preparation returned a different instance.");
            setPrepared(true);
          } finally { setPreparing(false); }
        }}>{preparing ? selectLocaleText(props.locale, "正在准备…", "Preparing…") : label}</InlineConfirmAction>
    </div>}</ConfigurationHelp>
    {prepared ? <ActivityNotice tone="success">{selectLocaleText(props.locale,
      "账号管理扩展已准备。启动服务器后可自动取得平台身份，并按所选玩家修改管理员和白名单。",
      "The account management extension is prepared. Start the server to collect platform identities and manage admins and the allowlist for the selected player.")}</ActivityNotice> : null}
  </section>;
}
