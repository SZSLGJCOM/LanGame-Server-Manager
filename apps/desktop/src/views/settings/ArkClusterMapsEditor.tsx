import { useId, useMemo, useState } from "react";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { useI18n } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { ArkMapAvailabilityNotice, ArkMapUnavailableWarning } from "./ArkMapAvailabilityNotice";
import { ConfigurationHelp } from "./ConfigurationFieldHelp";
import { ARK_MOD_MAP_OVERRIDES, readArkModMapOverrides } from "../servers/ark-map-mods";
import {
  ARK_ADDITIONAL_MAP_LIMIT, arkMapsText, arkMapSelectionErrorMessage, buildArkMapSelectionPatch, newArkMapId, readAdditionalArkMaps,
  readArkMapSuggestions, validateAdditionalArkMaps, type AdditionalArkMap
} from "./ark-cluster-maps";
import "./ark-cluster-maps.css";

export function ArkClusterMapsEditor(props: ConfigurationSpecializedRendererProps) {
  const { t, locale } = useI18n();
  const titleId = useId();
  const inputId = useId();
  const suggestions = useMemo(() => props.arkMapAvailability?.choices ?? readArkMapSuggestions(props.moduleDetails.schema_json, locale),
    [props.arkMapAvailability?.choices, props.moduleDetails.schema_json, locale]);
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
  const mainServerName = typeof props.settings.server_name === "string" ? props.settings.server_name : props.details.summary.name;
  const gamePort = (id: string | null) => {
    const port = props.details.ports.find((port) => port.name === (id ? `map-${id}-game` : "game"));
    return port ? `${port.port}/${port.protocol}` : arkMapsText(t, "automaticPorts");
  };
  const description = [arkMapsText(t, "shared"), arkMapsText(t, "names"),
    ...(props.arkMapAvailability ? [arkMapsText(t, "availableMaps")] : [])].join("\n\n");

  function patch(next: AdditionalArkMap[]) {
    if (disabled) return;
    const change: Record<string, unknown> = { additional_maps: next };
    if (props.settings[ARK_MOD_MAP_OVERRIDES] !== undefined) {
      try {
        const scopes = new Set(["main", ...next.map((map) => `map-${map.id}`)]);
        change[ARK_MOD_MAP_OVERRIDES] = Object.fromEntries(Object.entries(readArkModMapOverrides(props.settings, props.details.summary.module_id))
          .filter(([scope]) => scopes.has(scope)));
      } catch { setAddError(arkMapsText(t, "enableMapFailed")); return; }
    }
    props.onPatch(change);
  }
  function update(id: string, change: Partial<AdditionalArkMap>) {
    if (!maps) return;
    if (change.map_name !== undefined && !disabled) {
      try {
        props.onPatch(buildArkMapSelectionPatch(props.settings, change.map_name, suggestions,
          { additional_maps: maps.map((map) => map.id === id ? { ...map, ...change } : map) }, `map-${id}`));
        setAddError(null);
      } catch (cause) { setAddError(arkMapSelectionErrorMessage(cause, t)); }
      return;
    }
    patch(maps.map((map) => map.id === id ? { ...map, ...change } : map));
  }
  function add() {
    if (disabled || !maps || maps.length >= ARK_ADDITIONAL_MAP_LIMIT) return;
    const candidate = { id: newArkMapId(maps.map((map) => map.id)), map_name: mapName,
      name: newName.trim() || suggestedName, enabled: true };
    const error = validateAdditionalArkMaps({ additional_maps: [...maps, candidate] });
    if (error) { setAddError(arkMapsText(t, `error.${error}`)); return; }
    try {
      props.onPatch(buildArkMapSelectionPatch(props.settings, mapName, suggestions, { additional_maps: [...maps, candidate] }, `map-${candidate.id}`));
    } catch (cause) { setAddError(arkMapSelectionErrorMessage(cause, t)); return; }
    setNewName(""); setAddError(null);
  }

  return <section className="ark-editor ark-map-editor" data-field-key="additional_maps" aria-labelledby={titleId}>
    <div className="ark-editor__heading"><ConfigurationHelp description={description}>{(help) => <h3
      id={titleId} className="ark-map-editor__title-help" tabIndex={0} ref={help.anchorRef}
      {...help.interactionProps} aria-describedby={help.descriptionId}>{arkMapsText(t, "title")}</h3>}</ConfigurationHelp>
      <span className="ark-editor__hint">{arkMapsText(t, "count", { count: (maps?.length ?? 0) + 1 })}</span>
    </div>
    {props.arkMapAvailability ? <ArkMapAvailabilityNotice availability={props.arkMapAvailability} showDescription={false} /> : null}
    {running ? <p className="ark-editor__hint" role="status">{arkMapsText(t, "stopFirst")}</p> : null}
    {validation ? <p className="ark-editor__error" role="alert">{arkMapsText(t, `error.${validation}`)}</p> : null}
    <div className="ark-editor__table-scroll"><table className="ark-editor__table ark-map-editor__table">
      <thead><tr><th>{arkMapsText(t, "name")}</th><th>{arkMapsText(t, "map")}</th><th>{arkMapsText(t, "ports")}</th><th>{arkMapsText(t, "enabled")}</th><th>{arkMapsText(t, "actions")}</th></tr></thead>
      <tbody><tr><th scope="row"><input className="settings-schema-input" value={mainServerName} disabled={disabled}
          aria-label={arkMapsText(t, "primaryName")} onChange={(event) => {
            if (!disabled) props.onPatch({ server_name: event.target.value });
          }} /><small>{arkMapsText(t, "primary")}</small></th><td>{mainLabel}<code>{mainMap}</code>
          <ArkMapUnavailableWarning availability={props.arkMapAvailability} map={mainMap} /></td>
        <td className="ark-map-editor__game-port">{gamePort(null)}</td><td>{arkMapsText(t, "alwaysEnabled")}</td><td>—</td></tr>
        {(maps ?? []).map((map) => <tr key={map.id}>
          <th scope="row"><input className="settings-schema-input" value={map.name} disabled={disabled}
            aria-label={arkMapsText(t, "nameFor", { name: map.name || map.id })} onChange={(event) => update(map.id, { name: event.target.value })} /></th>
          <td>{savedIds.has(map.id) ? <><span>{suggestions.find((suggestion) => suggestion.value === map.map_name)?.label ?? map.map_name}</span><code>{map.map_name}</code></> :
            <input className="settings-schema-input" value={map.map_name} maxLength={128} disabled={disabled}
              aria-label={arkMapsText(t, "mapFor", { name: map.name || map.id })} onChange={(event) => update(map.id, { map_name: event.target.value })} />}
            <ArkMapUnavailableWarning availability={props.arkMapAvailability} map={map.map_name} /></td>
          <td className="ark-map-editor__game-port">{gamePort(map.id)}</td>
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
          {selected !== "__custom__" && !suggestions.some((suggestion) => suggestion.value === selected)
            ? <option value={selected} disabled>{selected}</option> : null}
          <option value="__custom__">{arkMapsText(t, "custom")}</option>
        </select></label>
      {selected === "__custom__" ? <label htmlFor={`${inputId}-custom`}>{arkMapsText(t, "package")}
        <input id={`${inputId}-custom`} className="settings-schema-input" value={customMap} maxLength={128} disabled={disabled}
          onChange={(event) => { setCustomMap(event.target.value); setAddError(null); }} />
        <small className="ark-editor__hint">{arkMapsText(t, "customInstalled")}</small></label> : null}
      <ArkMapUnavailableWarning availability={props.arkMapAvailability} map={mapName} />
      <label htmlFor={`${inputId}-name`}>{arkMapsText(t, "name")}
        <input id={`${inputId}-name`} className="settings-schema-input" value={newName} placeholder={suggestedName} disabled={disabled}
          onChange={(event) => { setNewName(event.target.value); setAddError(null); }} /></label>
      <button type="button" className="secondary-button" disabled={disabled || (maps?.length ?? 0) >= ARK_ADDITIONAL_MAP_LIMIT}
        onClick={add}>{arkMapsText(t, "add")}</button>
    </div>
    {addError ? <p className="ark-editor__error" role="alert">{addError}</p> : null}
    <p className="ark-editor__hint">{arkMapsText(t, "retained")}</p>
  </section>;
}
