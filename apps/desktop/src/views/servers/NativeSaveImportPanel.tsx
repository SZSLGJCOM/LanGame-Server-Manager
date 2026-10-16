import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import { localSaveImportAvailable, previewSaveImport, readSaveImportSupport,
  type SaveImportPreview, type SaveImportSupport } from "../../save-import";
import type { InstanceDetails } from "../../types";
import { useSaveImportOperation } from "./SaveImportOperations";

interface Props {
  details: InstanceDetails;
  onPickDirectory?: (currentPath?: string | null) => Promise<string | null>;
  onChanged?: () => void;
  formatHelp?: ReactNode;
}
interface CheckedSource { preview: SaveImportPreview; settingsJson: string }

export function NativeSaveImportPanel({ details, onPickDirectory, onChanged, formatHelp }: Props) {
  const { locale } = useI18n();
  const sourceId = useId();
  const hintId = useId();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const hostAvailable = localSaveImportAvailable();
  const running = Boolean(details.active_run) || instanceHasRunningProcess(details.summary, details.active_run)
    || ["Starting", "Stopping"].includes(details.summary.status);
  const importOperation = useSaveImportOperation(details.summary.id);
  const mounted = useRef(true);
  const supportRequest = useRef(0);
  const operation = useRef(false);
  const [support, setSupport] = useState<SaveImportSupport | null>(null);
  const [loading, setLoading] = useState(hostAvailable);
  const [source, setSource] = useState("");
  const [checked, setChecked] = useState<CheckedSource | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [localBusy, setBusy] = useState<"picking" | "checking" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = importOperation.pending ? "importing" : localBusy;
  const result = importOperation.result;
  const backupPaths = result?.safeguard_backup_paths?.length ? result.safeguard_backup_paths
    : result ? [result.safeguard_backup_path] : [];
  const visibleError = error ?? importOperation.error;
  const preview = checked?.settingsJson === details.settings_json ? checked.preview : null;
  const { id: instanceId, module_id: moduleId } = details.summary;
  const settingsJson = details.settings_json;

  // Native source hints include the world/slot selected in Configuration.
  const loadSupport = useCallback(async () => {
    if (!hostAvailable) return;
    const request = ++supportRequest.current;
    setLoading(true);
    setSupport(null);
    setError(null);
    try {
      const response = await readSaveImportSupport(instanceId, moduleId);
      if (mounted.current && supportRequest.current === request) setSupport(response);
    } catch (failure) {
      if (mounted.current && supportRequest.current === request) setError(describeError(failure));
    } finally {
      if (mounted.current && supportRequest.current === request) setLoading(false);
    }
  }, [hostAvailable, instanceId, moduleId, settingsJson]);
  useEffect(() => {
    mounted.current = true;
    void loadSupport();
    return () => { mounted.current = false; supportRequest.current++; };
  }, [loadSupport]);

  function replaceSource(value: string) {
    setSource(value);
    setChecked(null);
    setConfirmed(false);
    importOperation.clear();
    setError(null);
  }
  async function pick() {
    if (!onPickDirectory || operation.current || importOperation.pending) return;
    operation.current = true;
    setBusy("picking");
    try {
      const path = await onPickDirectory(source || null);
      if (mounted.current && path) replaceSource(path);
    } catch (failure) {
      if (mounted.current) setError(describeError(failure));
    } finally {
      operation.current = false;
      if (mounted.current) setBusy(null);
    }
  }
  async function inspect() {
    if (operation.current || importOperation.pending || running || !support?.supported || !source.trim()) return;
    operation.current = true;
    setBusy("checking");
    setChecked(null);
    setConfirmed(false);
    importOperation.clear();
    setError(null);
    try {
      const response = await previewSaveImport(instanceId, moduleId, source.trim());
      if (mounted.current) setChecked({ preview: response, settingsJson: details.settings_json });
    } catch (failure) {
      if (mounted.current) setError(describeError(failure));
    } finally {
      operation.current = false;
      if (mounted.current) setBusy(null);
    }
  }
  async function performImport() {
    if (operation.current || importOperation.pending || running || !preview || !confirmed || !support?.supported) return;
    operation.current = true;
    setError(null);
    try {
      await importOperation.run(preview.source_path, preview.fingerprint);
      if (mounted.current) {
        setChecked(null);
        setConfirmed(false);
      }
    } catch (failure) {
      if (mounted.current) { setError(describeError(failure)); setChecked(null); setConfirmed(false); }
    } finally {
      operation.current = false;
    }
    onChanged?.();
  }

  if (!hostAvailable) return <p className="form-note" role="status">{text(
    "请在运行此服务器的 LGSM 桌面端导入本机存档。网页连接和界面预览无法访问本机存档。",
    "Import local saves from the LGSM desktop on the server host. Web connections and UI previews cannot access local saves.")}</p>;
  return <div className="save-import__operation" aria-busy={Boolean(busy) || loading}>
    {loading ? <p className="form-note" role="status">{text("正在读取导入能力…", "Reading import support…")}</p> : null}
    {!loading && !support && error ? <button type="button" className="secondary-button" onClick={() => void loadSupport()}>
      {text("重新读取导入能力", "Retry reading import support")}</button> : null}
    {support && !support.supported ? <p className="save-import__availability" role="status">
      <ShellIcon name="inbox" aria-hidden="true" />
      {moduleId === "squad" ? text("此游戏没有可导入的持久世界存档。", "This game has no persistent world save to import.")
        : text("当前游戏不支持在这里导入存档。", "Save import is unavailable for this game.")}</p> : null}
    {support?.supported ? <>
      <div className="save-import__availability" role="status"><ShellIcon name="check-circle" aria-hidden="true" />
        <span>{text("支持导入已有存档", "Existing saves can be imported")}</span>
        <span className="save-import__safeguards">{text("自动备份 · 保留来源 · 保留服务器配置", "Automatic backup · Source retained · Server configuration retained")}</span>
      </div>
      <div className="save-import__source-field">
        <label className="save-import__source-label" htmlFor={sourceId}>{text("来源存档目录", "Source save directory")}</label>
        <div className="save-import__source-row">
        <input id={sourceId} type="text" value={source} disabled={Boolean(busy)} autoComplete="off" spellCheck={false}
          aria-describedby={hintId} onChange={event => replaceSource(event.target.value)}
          onKeyDown={event => { if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); void inspect(); } }}
          placeholder={text("输入路径或选择文件夹", "Enter a path or choose a folder")} />
        {onPickDirectory ? <button type="button" className="secondary-button" disabled={Boolean(busy)} onClick={() => void pick()}>
          {text("选择文件夹", "Choose folder")}</button> : null}
        <button type="button" className="secondary-button" disabled={Boolean(busy) || running || !source.trim()} onClick={() => void inspect()}>
          {busy === "checking" ? text("检查中…", "Checking…") : text("检查存档", "Check save")}</button>
        </div>
        <p id={hintId} className="form-note">{text("先关闭来源游戏或服务器，选择完整存档目录。", "Close the source game or server, then select its complete save directory.")}</p>
      </div>
      <details className="save-import__format"><summary>{text("格式要求", "Format requirements")}</summary>
        <div className="save-import__format-body">
          {formatHelp}
          <p>{text("传输压缩包请先解压，游戏原生存档容器保持原格式。世界名称或槽位须与服务器设置一致。",
            "Extract transport archives; retain game-native save containers. Match the world name or slot in server settings.")}</p>
          {support.source_hint ? <p className="save-import__native-hint"><span>{text("当前配置要求", "Requirements for the current configuration")}</span>{support.source_hint}</p> : null}
          {preview?.warnings?.filter(warning => !warning.startsWith("Import replaces the destination save directory"))
            .map((warning, index) => <p className="form-note" key={index}>{warning}</p>)}
        </div>
      </details>
      {running ? <p className="form-note" role="status">{text("请先停止当前服务器，再检查和导入存档。", "Stop this server before checking or importing a save.")}</p> : null}
      {preview ? <div className="save-import__preview">
        <div className="save-import__preview-heading"><h4>{text("检查完成", "Ready to import")}</h4>
          <span>{preview.file_count.toLocaleString(locale)} {text("个文件", "files")} · {formatBytes(preview.total_bytes, locale)}</span></div>
        <dl className="save-import__paths">
          <div><dt>{text("来源", "Source")}</dt><dd><code>{preview.source_path}</code></dd></div>
          <div><dt>{text("目标", "Target")}</dt><dd><code>{preview.target_path}</code></dd></div>
        </dl>
        {preview.changes?.length ? <div className="save-import__scope">
          <span>{text("导入范围", "Import scope")}</span>
          <ul className="save-import__changes">{preview.changes.map(change => <li key={change.path}>
            <code>{change.path === "." ? text("此存档目录", "This save directory") : change.path}</code>
            <span>{change.file_count.toLocaleString(locale)} {text("个文件", "files")} · {formatBytes(change.total_bytes, locale)}</span>
          </li>)}</ul>
        </div> : null}
        <p className="form-note">{text("当前存档", "Current saves")} {preview.existing_file_count.toLocaleString(locale)} {text("个文件", "files")} · {formatBytes(preview.existing_total_bytes, locale)}</p>
        <label className="save-import__confirm"><input type="checkbox" checked={confirmed} disabled={Boolean(busy) || running}
          onChange={event => setConfirmed(event.target.checked)} />
          <span>{text("确认按此游戏的规则更新以上位置的世界和玩家存档。导入前自动备份，保留来源与服务器配置。",
            "Update world and player saves at these locations using this game's import rules. Back up first and retain the source and server configuration.")}</span></label>
        <button type="button" className="primary-button" disabled={!confirmed || Boolean(busy) || running} onClick={() => void performImport()}>
          {busy === "importing" ? text("正在导入…", "Importing…") : text("备份并导入", "Back up and import")}</button>
      </div> : null}
    </> : null}
    {busy === "importing" ? <p className="form-note" role="status">{text("正在备份并导入，请等待结果。切换选项卡不会取消此操作。",
      "Backing up and importing. Wait for the result; switching tabs does not cancel this operation.")}</p> : null}
    {visibleError ? <p className="save-import__error" role="alert">{visibleError}</p> : null}
    {result ? <div className="save-import__result" role="status">
      <div className="save-import__result-heading"><ShellIcon name="check-circle" aria-hidden="true" /><h4>{text("存档已导入", "Save imported")}</h4>
        <span>{text("等待启动验证", "Startup verification required")}</span></div>
      <p>{result.copied_file_count.toLocaleString(locale)} {text("个文件", "files")} · {formatBytes(result.copied_total_bytes, locale)} · {text("启动后核对世界与玩家数据。", "Verify the world and players after starting.")}</p>
      {result.warnings.map((warning, index) => <p className="save-import__error" key={index}>{warning}</p>)}
      <dl className="save-import__paths"><div><dt>{text("存档位置", "Save location")}</dt><dd><code>{result.saves_path}</code></dd></div>
        <div><dt>{text("导入前备份", "Pre-import backup")}</dt><dd className="save-import__backup-paths">
          {backupPaths.map(path => <code key={path}>{path}</code>)}
        </dd></div></dl>
      <p>{backupPaths.length > 1
        ? text("需要回退时，停服后在「存档与备份」恢复以上全部备份。", "To roll back, stop the server and restore every backup listed above under Saves and backups.")
        : text("需要回退时，停服后在「存档与备份」恢复。", "To roll back, stop the server and restore under Saves and backups.")}</p>
    </div> : null}
  </div>;
}

function formatBytes(bytes: number, locale: string): string {
  const unit = bytes > 0 ? Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), 4) : 0;
  return `${(bytes / 1024 ** unit).toLocaleString(locale, { maximumFractionDigits: 1 })} ${["B", "KiB", "MiB", "GiB", "TiB"][unit]}`;
}
