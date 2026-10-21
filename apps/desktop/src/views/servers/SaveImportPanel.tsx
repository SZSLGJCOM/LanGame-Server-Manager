import { useState } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { openExternalUrl } from "../../api";
import { selectLocaleText, useI18n } from "../../i18n";
import { findSaveImportGuide, saveGuideText } from "../../save-import/guides";
import type { DstWorldImportResult, InstanceDetails } from "../../types";
import { DstWorldImportPanel } from "./DstWorldImportPanel";
import { MaintenanceChip } from "./MaintenanceToolbar";
import { NativeSaveImportPanel } from "./NativeSaveImportPanel";
import "./save-import.css";

interface Props {
  details: InstanceDetails;
  readOnly: boolean;
  onPickDirectory?: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData?: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onChanged?: () => void;
}

/** 「导入」 in the saves strip: a popover flow (source → check → confirm) whose guidance lives in help bubbles. */
export function SaveImportPanel(props: Props) {
  const { locale } = useI18n();
  const [referenceError, setReferenceError] = useState<string | null>(null);
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const guide = findSaveImportGuide(props.details.summary.module_id);
  const dst = props.details.summary.module_id === "dontstarve";
  const singlePlayerNote = guide?.singlePlayer.status === "direct"
    ? text("本地世界可迁入，进入游戏后核对角色与玩家数据。", "Local worlds can be transferred; verify characters and player data in game.")
    : guide?.singlePlayer.status === "unsupported"
      ? text("此游戏不支持单机存档转专服。", "This game does not support converting single-player saves to a dedicated server.")
      : guide?.singlePlayer.status === "unverified"
        ? text("请先准备专服可读取的存档；本功能不会转换单机角色或原房主身份。", "Prepare a server-compatible save first; this import does not convert single-player characters or the original host's identity.")
        : guide?.singlePlayer.status === "conditional" ? saveGuideText(guide.singlePlayer, locale) : null;
  const guidance = [
    singlePlayerNote ? `${text("单机存档", "Single-player saves")}：${singlePlayerNote}` : null,
    guide ? saveGuideText(guide.source, locale) : null,
    ...(guide?.cautions.map(note => `· ${saveGuideText(note, locale)}`) ?? [])
  ].filter(Boolean).join("\n");
  const label = text("导入", "Import");
  return <MaintenanceChip icon="inbox" label={label} className="save-import" lazy={!props.readOnly} help={guidance || null}>
    {props.readOnly ? <p className="form-note" role="status">{text("请先恢复此实例，再导入存档。", "Restore this instance before importing a save.")}</p>
      : dst && props.onPickDirectory && props.onImportDontStarveWorldData ? <DstWorldImportPanel details={props.details}
        onPickDirectory={props.onPickDirectory} onImportWorldData={props.onImportDontStarveWorldData} />
        : dst ? <p className="form-note" role="status">{text("当前连接未提供世界导入操作。", "World import is unavailable on this connection.")}</p>
          : <NativeSaveImportPanel key={props.details.summary.id} details={props.details}
            onPickDirectory={props.onPickDirectory} onChanged={props.onChanged} />}
    {guide && guide.references.length > 0 ? <div className="save-import__references">
      {guide.references.map(reference => <a key={reference.url} href={reference.url} target="_blank" rel="noreferrer noopener"
        onClick={event => {
          event.preventDefault();
          setReferenceError(null);
          void openExternalUrl(reference.url).catch(() => setReferenceError(text("无法打开来源链接，请复制链接后在浏览器中打开。",
            "Could not open the reference. Copy the link and open it in your browser.")));
        }}>{reference.label}</a>)}
    </div> : null}
    <ActivityNotice tone="error">{referenceError ?? ""}</ActivityNotice>
  </MaintenanceChip>;
}
