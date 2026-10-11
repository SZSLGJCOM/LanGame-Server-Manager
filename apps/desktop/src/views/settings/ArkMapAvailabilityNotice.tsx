import { useI18n } from "../../i18n";
import { describeError } from "../../app-state";
import { arkMapsText, type ArkMapAvailabilityView } from "./ark-cluster-maps";

export function ArkMapUnavailableWarning(props: { availability?: ArkMapAvailabilityView; map: string }) {
  const { t } = useI18n();
  if (props.availability?.status !== "ready" || !props.map
    || props.availability.choices.some((choice) => choice.value === props.map)) return null;
  return <small className="ark-editor__error" role="status" data-ark-map-unavailable={props.map}>
    {arkMapsText(t, "unavailableMap", { map: props.map })}
  </small>;
}

export function ArkMapAvailabilityNotice(props: { availability: ArkMapAvailabilityView; showDescription?: boolean }) {
  const { t } = useI18n();
  const { status, refresh } = props.availability;
  return <div className="ark-map-availability" aria-busy={status === "loading"}>
    {props.showDescription !== false ? <p className="ark-editor__hint">{arkMapsText(t, "availableMaps")}</p> : null}
    {status !== "ready" ? <p className="ark-editor__hint" role="status">
      {arkMapsText(t, status === "loading" ? "loadingMaps" : "readMapsFailed")}
    </p> : null}
    {status === "failed" && props.availability.error ? <details>
      <summary>{arkMapsText(t, "readMapsErrorDetails")}</summary>
      <p className="ark-editor__error">{describeError(props.availability.error)}</p>
    </details> : null}
    <button type="button" className="ghost-button" disabled={status === "loading"} onClick={refresh}>
      {arkMapsText(t, "reloadMaps")}
    </button>
  </div>;
}
