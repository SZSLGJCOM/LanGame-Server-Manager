import { useId, useState } from "react";
import { openExternalUrl } from "../../api";
import { selectLocaleText, useI18n } from "../../i18n";
import { findSaveImportGuide, saveGuideText } from "../../save-import/guides";
import type { DstWorldImportResult, InstanceDetails } from "../../types";
import { DstWorldImportPanel } from "./DstWorldImportPanel";
import { NativeSaveImportPanel } from "./NativeSaveImportPanel";
import "./save-import.css";

interface Props {
  details: InstanceDetails;
  readOnly: boolean;
  onPickDirectory?: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData?: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onChanged?: () => void;
}

export function SaveImportPanel(props: Props) {
  const { locale } = useI18n();
  const titleId = useId();
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
  const formatHelp = guide ? <>
    <p>{saveGuideText(guide.source, locale)}</p>
    {guide.cautions.length > 0 ? <ul>{guide.cautions.map((note, index) => <li key={index}>{saveGuideText(note, locale)}</li>)}</ul> : null}
    {guide.references.length > 0 ? <div className="save-import__references">
      {guide.references.map(reference => <a key={reference.url} href={reference.url} target="_blank" rel="noreferrer noopener"
        onClick={event => {
          event.preventDefault();
          setReferenceError(null);
          void openExternalUrl(reference.url).catch(() => setReferenceError(text("无法打开来源链接，请复制链接后在浏览器中打开。",
            "Could not open the reference. Copy the link and open it in your browser.")));
        }}>{reference.label}</a>)}
      {referenceError ? <p role="alert">{referenceError}</p> : null}
    </div> : null}
  </> : null;
  return <section className="server-workbench-surface server-maintenance-card save-import" aria-labelledby={titleId}>
    <h3 id={titleId} className="server-maintenance-card-label save-import__title">{text("导入存档", "Import save")}</h3>
    {singlePlayerNote ? <p className="save-import__limitation"><span>{text("单机存档", "Single-player saves")}</span>{singlePlayerNote}</p> : null}
    {props.readOnly ? <p className="form-note" role="status">{text("请先恢复此实例，再导入存档。", "Restore this instance before importing a save.")}</p>
      : dst && props.onPickDirectory && props.onImportDontStarveWorldData ? <DstWorldImportPanel details={props.details}
        onPickDirectory={props.onPickDirectory} onImportWorldData={props.onImportDontStarveWorldData} />
        : dst ? <p className="form-note" role="status">{text("当前连接未提供世界导入操作。", "World import is unavailable on this connection.")}</p>
          : <NativeSaveImportPanel key={props.details.summary.id} details={props.details}
            onPickDirectory={props.onPickDirectory} onChanged={props.onChanged} formatHelp={formatHelp} />}
    {dst && formatHelp ? <details className="save-import__format"><summary>{text("格式要求", "Format requirements")}</summary>
      <div className="save-import__format-body">{formatHelp}</div></details> : null}
  </section>;
}
