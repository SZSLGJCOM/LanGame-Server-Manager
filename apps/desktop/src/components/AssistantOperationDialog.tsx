import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { assistantConfirmationExpired, canContinueAssistantTask } from "../hooks/useAssistantOperationConfirmation";
import type { AssistantExecuteOperationOutput } from "../types";
import { containAssistantFocus } from "./assistant-focus";
import { AssistantFileDiff } from "./AssistantFileDiff";
import "./assistant-operation-dialog.css";

interface AssistantOperationDialogProps {
  preview: AssistantExecuteOperationOutput;
  onRespond: (confirmed: boolean, continueTask?: boolean) => void;
}

export function AssistantOperationDialog({ preview, onRespond }: AssistantOperationDialogProps) {
  const { t } = useI18n();
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [now, setNow] = useState(Date.now);
  const [continueTask, setContinueTask] = useState(false);
  const expired = assistantConfirmationExpired(preview, now);
  const change = preview.fileChangePreview;
  const batch = preview.fileChangePreviews ?? [];
  const validBatch = Array.isArray(batch) && batch.length > 0 && batch.length <= 8
    && batch.every((file) => typeof file.file === "string" && file.file.length > 0 && Array.isArray(file.edits)
      && file.edits.length > 0 && file.edits.every((edit) => typeof edit.before === "string" && typeof edit.after === "string"));
  const missingChange = (preview.action === "patch_instance_text" && !change)
    || (preview.action === "patch_instance_files" && !validBatch);
  const lifecycleAction = ["stop_server", "restart_server", "create_backup", "restore_backup"].includes(preview.action);

  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    const previous = document.activeElement;
    node.showModal();
    cancel.current?.focus();
    return () => {
      node.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  useEffect(() => {
    const deadline = preview.confirmationExpiresAtUnixMs;
    if (deadline == null || expired) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.min(Math.max(deadline - Date.now(), 0), 60_000));
    return () => window.clearTimeout(timer);
  }, [preview.confirmationExpiresAtUnixMs, expired, now]);

  return (
    <dialog ref={dialog} className="assistant-operation-dialog" aria-labelledby="assistant-operation-title"
      aria-describedby="assistant-operation-summary" data-no-window-drag="true"
      tabIndex={-1} onKeyDown={(event) => {
        if (event.key === "Tab" && dialog.current) containAssistantFocus(event, dialog.current, document.activeElement);
      }}
      onCancel={(event) => { event.preventDefault(); onRespond(false); }}
      onClose={() => { if (!dialog.current?.open) onRespond(false); }}>
      <header>
        <h2 id="assistant-operation-title">{lifecycleAction ? t(`assistant.tools.${preview.action}`) : t("assistant.operation.dialog.title")}</h2>
      </header>
      <div className="assistant-operation-body">
        <p id="assistant-operation-summary" className="assistant-operation-summary">{preview.planSummary}</p>
        {lifecycleAction ? <p className="form-note">{t(`assistant.operation.dialog.${preview.action}`)}</p> : null}
        {(change || validBatch) ? <p className="form-note">{t("assistant.operation.dialog.backup")}</p> : null}
        {preview.action === "patch_instance_text" && change ? <AssistantFileDiff file={change.file} edits={[change]} index={0} /> : null}
        {preview.action === "patch_instance_files" && validBatch ? batch.map((file, index) =>
          <AssistantFileDiff key={file.file} file={file.file} edits={file.edits} index={index} />) : null}
        {(change || validBatch) ? <>
          <p className="form-note">{t("assistant.operation.dialog.conflict")}</p>
          <p className="form-note">{t("assistant.operation.dialog.runtimeUnverified")}</p>
        </> : null}
        {missingChange ? <p role="alert" className="form-note error">{t("assistant.operation.dialog.missingChange")}</p> : null}
        {expired ? <p role="alert" className="form-note error">{t("assistant.operation.dialog.expired")}</p> : null}
      </div>
      <footer>
        {canContinueAssistantTask(preview) ? <section className="assistant-task-authorization">
          <label><input type="checkbox" checked={continueTask} disabled={expired || missingChange}
            onChange={(event) => setContinueTask(event.target.checked)} />{t("assistant.operation.dialog.continueTask")}</label>
          <p className="form-note">{t("assistant.operation.dialog.continueScope")}</p>
        </section> : null}
        <button ref={cancel} type="button" className="ghost-button" onClick={() => onRespond(false)}>
          {t("assistant.operation.dialog.cancel")}
        </button>
        <button type="button" className="secondary-button" disabled={expired || missingChange}
          onClick={() => onRespond(!assistantConfirmationExpired(preview) && !missingChange, continueTask)}>
          {t("assistant.operation.dialog.confirm")}
        </button>
      </footer>
    </dialog>
  );
}
