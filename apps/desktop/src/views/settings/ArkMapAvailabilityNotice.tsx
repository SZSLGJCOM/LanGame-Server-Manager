import { useI18n } from "../../i18n";
import { describeError } from "../../app-state";
import { ActivityNotice } from "../../components/ActivityNotice";
import { arkMapsText, type ArkMapAvailabilityView } from "./ark-cluster-maps";
import { focusConfigurationControl } from "./configuration-workspace-state";

export function ArkMapUnavailableWarning(props: { availability?: ArkMapAvailabilityView; map: string; inputId?: string }) {
  const { t } = useI18n();
  if (props.availability?.status !== "ready" || !props.map
    || props.availability.choices.some((choice) => choice.value === props.map)) return null;
  return <ActivityNotice tone="warning" action={<button type="button" className="secondary-button configuration-validation-target"
    data-ark-map-unavailable={props.map} aria-controls={props.inputId}
    onClick={() => props.inputId ? focusConfigurationControl(props.inputId) : props.availability?.refresh()}>
    {props.inputId ? arkMapsText(t, "mapFor", { name: props.map }) : arkMapsText(t, "reloadMaps")}
  </button>}>
    {arkMapsText(t, "unavailableMap", { map: props.map })}
  </ActivityNotice>;
}

export function ArkMapAvailabilityNotice(props: { availability: ArkMapAvailabilityView; showDescription?: boolean }) {
  const { t } = useI18n();
  const { status, refresh } = props.availability;
  return <div className="ark-map-availability" aria-busy={status === "loading"}>
    {props.showDescription !== false ? <p className="ark-editor__hint">{arkMapsText(t, "availableMaps")}</p> : null}
    {status === "loading" ? <ActivityNotice>
      {arkMapsText(t, "loadingMaps")}
    </ActivityNotice> : null}
    {status === "failed" ? <ActivityNotice tone="error" details={props.availability.error ? describeError(props.availability.error) : undefined}
      action={<button type="button" className="secondary-button" onClick={refresh}>{arkMapsText(t, "reloadMaps")}</button>}>
      {arkMapsText(t, "readMapsFailed")}
    </ActivityNotice> : null}
    <button type="button" className="ghost-button" disabled={status === "loading"} onClick={refresh}>
      {arkMapsText(t, "reloadMaps")}
    </button>
  </div>;
}
