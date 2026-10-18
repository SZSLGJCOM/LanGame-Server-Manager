import { useState } from "react";
import { useI18n } from "../../i18n";
import { openExternalUrl } from "../../api";
import type { NexusModReview } from "../../nexus-mods-api";
import { NexusDialog } from "./NexusDialog";
import { nexusText } from "./nexus-mods-messages";
import { formatWorkshopByteSize } from "./steam-workshop-store-model";

export function NexusFileSelectionDialog(props: {
  reviews: NexusModReview[]; busy: boolean; disabled: boolean; error: string | null; installNote?: string | null;
  onClose(): void; onInstall(selections: Record<number, string>): void;
}) {
  const { locale } = useI18n(), text = (key: Parameters<typeof nexusText>[1]) => nexusText(locale, key);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Record<number, string>>(() => Object.fromEntries(props.reviews.flatMap((review) =>
    review.selected_file_id && review.files.some((file) => file.file_id === review.selected_file_id && file.downloadable)
      ? [[review.reference_index, review.selected_file_id]] : [])));
  const valid = props.reviews.length > 0 && props.reviews.every((review) => review.files.some((file) => file.file_id === selected[review.reference_index] && file.downloadable));
  return <NexusDialog title={text("pickerTitle")} closeLabel={text("cancel")} busy={props.busy} onClose={props.onClose} className="nexus-file-selection">
    <div className="nexus-dialog__body" data-nexus-file-review>
      <p>{text("pickerHelp")}</p>
      {props.installNote ? <p className="nexus-file-selection__requirements">{props.installNote}</p> : null}
      {props.reviews.map((review) => <fieldset key={review.reference_index} className="nexus-file-selection__project" data-nexus-project={review.reference_index} disabled={props.busy || props.disabled}>
        <legend>{review.mod_name || review.mod_id}</legend>
        <p className="nexus-dialog__muted">{review.game_domain} · {review.mod_id}</p>
        {/^https:\/\/(www\.)?nexusmods\.com\//.test(review.page_url) ? <a href={review.page_url} onClick={(event) => {
          event.preventDefault(); void openExternalUrl(review.page_url).catch(() => setLinkError(text("openFailed")));
        }}>{text("projectPage")}</a> : null}
        {review.files.length ? review.files.map((file) => <label key={file.file_id} className={`nexus-file-selection__file${file.downloadable ? "" : " is-unavailable"}`}>
          <input type="radio" name={`nexus-file-${review.reference_index}`} value={file.file_id} checked={selected[review.reference_index] === file.file_id}
            disabled={!file.downloadable || props.busy || props.disabled} data-nexus-file-id={file.file_id} data-nexus-reference-index={review.reference_index}
            onChange={() => setSelected((previous) => ({ ...previous, [review.reference_index]: file.file_id }))} />
          <span className="nexus-file-selection__copy"><strong>{file.name || file.file_name}</strong>
            <span className="nexus-dialog__muted">{[file.file_name, file.version, file.category, formatWorkshopByteSize(file.size_bytes)].filter(Boolean).join(" · ")}</span>
            {file.description ? <span className="nexus-file-selection__description">{file.description}</span> : null}
            {!file.downloadable ? <span className="nexus-dialog__error">{file.unavailable_reason || text("unavailable")}</span> : null}
          </span>
        </label>) : <p>{text("noFiles")}</p>}
      </fieldset>)}
      {props.error ? <p role="alert" className="nexus-dialog__error">{props.error}</p> : null}
      {linkError ? <p role="alert" className="nexus-dialog__error">{linkError}</p> : null}
    </div>
    <footer className="nexus-dialog__footer">
      {!valid ? <span className="nexus-dialog__muted">{text("selectRequired")}</span> : null}
      <button type="button" className="mw-ghost-btn" data-nexus-cancel disabled={props.busy} onClick={props.onClose}>{text("cancel")}</button>
      <button type="button" className="mw-btn mw-btn--primary" data-nexus-install disabled={!valid || props.busy || props.disabled}
        onClick={() => props.onInstall(selected)}>{text(props.busy ? "busy" : "install")}</button>
    </footer>
  </NexusDialog>;
}
