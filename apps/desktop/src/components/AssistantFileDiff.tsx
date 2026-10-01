import { useI18n } from "../i18n";

interface AssistantFileDiffProps {
  file: string;
  edits: Array<{ before: string; after: string }>;
  index: number;
}

export function AssistantFileDiff({ file, edits, index }: AssistantFileDiffProps) {
  const { t } = useI18n();
  return <section className="assistant-operation-change" aria-label={file}>
    <p className="assistant-operation-file-label">{t("assistant.operation.dialog.file")}</p>
    <code className="assistant-operation-file">{file}</code>
    {edits.map((edit, editIndex) => {
      const id = `assistant-operation-${index}-${editIndex}`;
      return <div className="assistant-operation-fragments" key={id}>
        <section aria-labelledby={`${id}-before`}>
          <h3 id={`${id}-before`}>{t("assistant.operation.dialog.before")}</h3>
          <pre tabIndex={0}><code>{edit.before}</code></pre>
          {edit.before === "" ? <span className="form-note">{t("assistant.operation.dialog.empty")}</span> : null}
        </section>
        <section aria-labelledby={`${id}-after`}>
          <h3 id={`${id}-after`}>{t("assistant.operation.dialog.after")}</h3>
          <pre tabIndex={0}><code>{edit.after}</code></pre>
          {edit.after === "" ? <span className="form-note">{t("assistant.operation.dialog.empty")}</span> : null}
        </section>
      </div>;
    })}
  </section>;
}
