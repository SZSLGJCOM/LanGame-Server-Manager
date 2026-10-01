import { useId, useMemo, useState } from "react";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { useI18n } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import {
  ARK_ADDITIONAL_MAP_LIMIT, arkMapsText, newArkMapId, readAdditionalArkMaps,
  readArkMapSuggestions, validateAdditionalArkMaps, type AdditionalArkMap
} from "./ark-cluster-maps";
import "./ark-cluster-maps.css";

export function ArkClusterMapsEditor(props: ConfigurationSpecializedRendererProps) {
  const { t, locale } = useI18n();
  const titleId = useId();
  const inputId = useId();
  const suggestions = useMemo(() => readArkMapSuggestions(props.moduleDetails.schema_json, locale), [props.moduleDetails.schema_json, locale]);
  const maps = readAdditionalArkMaps(props.settings);
  const validation = validateAdditionalArkMaps(props.settings);
  const [selectedMap, setSelectedMap] = useState("");
  const [customMap, setCustomMap] = useState("");
  const [newName, setNewName] = useState("");
  const [addError, setAddError] = useState<string | null>(null);
  const savedSettings: unknown = JSON.parse(props.details.settings_json);
  const savedMaps = savedSettings && typeof savedSettings === "object" && !Array.isArray(savedSettings)
    ? readAdditionalArkMaps(savedSettings as Record<string, unknown>) : null;
  const savedIds = new Set((savedMaps ?? []).map((map) => map.id));
  const running = instanceHasRunningProcess(props.details.summary, props.details.active_run)
    || ["starting", "stopping"].includes(String(props.details.summary.status).toLowerCase());
  const disabled = props.disabled || running || !maps;
  const defaultMap = suggestions.find((suggestion) => suggestion.value !== props.settings.map_name)?.value ?? suggestions[0]?.value ?? "__custom__";
  const selected = selectedMap || defaultMap;
  const mapName = selected === "__custom__" ? customMap.trim() : selected;
  const suggestedName = suggestions.find((suggestion) => suggestion.value === mapName)?.label ?? mapName;
  const mainMap = String(props.settings.map_name ?? "");
  const mainLabel = suggestions.find((suggestion) => suggestion.value === mainMap)?.label ?? mainMap;
  const portNames = (id: string | null) => props.details.ports
    .filter((port) => id ? ["game", "peer", "query", "rcon"].some((name) => port.name === `map-${id}-${name}`) : !port.name.startsWith("map-"))
    .map((port) => `${id ? port.name.slice(`map-${id}-`.length) : port.name}: ${port.port}/${port.protocol}`);

  function patch(next: AdditionalArkMap[]) {
    if (disabled) return;
    props.onPatch({ additional_maps: next });
  }
  function update(id: string, change: Partial<AdditionalArkMap>) {
    if (!maps) return;
    patch(maps.map((map) => map.id === id ? { ...map, ...change } : map));
  }
  function add() {
    if (disabled || !maps || maps.length >= ARK_ADDITIONAL_MAP_LIMIT) return;
    const candidate = { id: newArkMapId(maps.map((map) => map.id)), map_name: mapName,
      name: newName.trim() || suggestedName, enabled: true };
    const error = validateAdditionalArkMaps({ additional_maps: [...maps, candidate] });
    if (error) { setAddError(arkMapsText(t, `error.${error}`)); return; }
    patch([...maps, candidate]);
    setNewName(""); setAddError(null);
  }

  return <section className="ark-editor ark-map-editor" data-field-key="additional_maps" aria-labelledby={titleId}>
    <div className="ark-editor__heading"><h3 id={titleId}>{arkMapsText(t, "title")}</h3>
      <span className="ark-editor__hint">{arkMapsText(t, "count", { count: (maps?.length ?? 0) + 1 })}</span>
    </div>
    <p className="ark-editor__hint">{arkMapsText(t, "shared")}</p>
    {running ? <p className="ark-editor__hint" role="status">{arkMapsText(t, "stopFirst")}</p> : null}
    {validation ? <p className="ark-editor__error" role="alert">{arkMapsText(t, `error.${validation}`)}</p> : null}
    <div className="ark-editor__table-scroll"><table className="ark-editor__table ark-map-editor__table">
      <thead><tr><th>{arkMapsText(t, "name")}</th><th>{arkMapsText(t, "map")}</th><th>{arkMapsText(t, "ports")}</th><th>{arkMapsText(t, "enabled")}</th><th>{arkMapsText(t, "actions")}</th></tr></thead>
      <tbody><tr><th scope="row">{arkMapsText(t, "primary")}</th><td>{mainLabel}<code>{mainMap}</code></td>
        <td>{portNames(null).join(" · ") || arkMapsText(t, "automaticPorts")}</td><td>{arkMapsText(t, "alwaysEnabled")}</td><td>—</td></tr>
        {(maps ?? []).map((map) => <tr key={map.id}>
          <th scope="row"><input className="settings-schema-input" value={map.name} maxLength={160} disabled={disabled}
            aria-label={arkMapsText(t, "nameFor", { name: map.name || map.id })} onChange={(event) => update(map.id, { name: event.target.value })} /></th>
          <td>{savedIds.has(map.id) ? <><span>{suggestions.find((suggestion) => suggestion.value === map.map_name)?.label ?? map.map_name}</span><code>{map.map_name}</code></> :
            <input className="settings-schema-input" value={map.map_name} maxLength={128} disabled={disabled}
              aria-label={arkMapsText(t, "mapFor", { name: map.name || map.id })} onChange={(event) => update(map.id, { map_name: event.target.value })} />}
            <small>{arkMapsText(t, "identity", { id: map.id })}</small></td>
          <td>{portNames(map.id).join(" · ") || arkMapsText(t, "automaticPorts")}</td>
          <td><input type="checkbox" checked={map.enabled} disabled={disabled}
            aria-label={arkMapsText(t, "enabledFor", { name: map.name || map.id })} onChange={(event) => update(map.id, { enabled: event.target.checked })} /></td>
          <td><InlineConfirmAction className="ghost-button" disabled={disabled} scopeKey={`${props.details.summary.id}:${map.id}`}
            aria-label={arkMapsText(t, "removeFor", { name: map.name || map.id })}
            confirmation={arkMapsText(t, "removeConfirm", { name: map.name || map.id })}
            onConfirm={() => patch((maps ?? []).filter((entry) => entry.id !== map.id))}>{arkMapsText(t, "remove")}</InlineConfirmAction></td>
        </tr>)}
      </tbody>
    </table></div>
    <div className="ark-map-editor__add">
      <label htmlFor={`${inputId}-map`}>{arkMapsText(t, "map")}
        <select id={`${inputId}-map`} className="settings-schema-input" value={selected} disabled={disabled || (maps?.length ?? 0) >= ARK_ADDITIONAL_MAP_LIMIT}
          onChange={(event) => { setSelectedMap(event.target.value); setAddError(null); }}>
          {suggestions.map((suggestion) => <option key={suggestion.value} value={suggestion.value}>{suggestion.label}</option>)}
          <option value="__custom__">{arkMapsText(t, "custom")}</option>
        </select></label>
      {selected === "__custom__" ? <label htmlFor={`${inputId}-custom`}>{arkMapsText(t, "package")}
        <input id={`${inputId}-custom`} className="settings-schema-input" value={customMap} maxLength={128} disabled={disabled}
          onChange={(event) => { setCustomMap(event.target.value); setAddError(null); }} /></label> : null}
      <label htmlFor={`${inputId}-name`}>{arkMapsText(t, "name")}
        <input id={`${inputId}-name`} className="settings-schema-input" value={newName} placeholder={suggestedName} maxLength={160} disabled={disabled}
          onChange={(event) => { setNewName(event.target.value); setAddError(null); }} /></label>
      <button type="button" className="secondary-button" disabled={disabled || (maps?.length ?? 0) >= ARK_ADDITIONAL_MAP_LIMIT}
        onClick={add}>{arkMapsText(t, "add")}</button>
    </div>
    {addError ? <p className="ark-editor__error" role="alert">{addError}</p> : null}
    <p className="ark-editor__hint">{arkMapsText(t, "retained")}</p>
  </section>;
}
