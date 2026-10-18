import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { describeError } from "../../app-state";
import type { SettingsObject } from "../settings/settings-schema";
import { AsaModConfiguration } from "./AsaModConfiguration";
import { ASA_MOD_INI_FILES, readAsaModConfigurations, type AsaModIniFile } from "./asa-mod-configuration";
import { arkModEnabledMaps, arkModHasMembershipOverrides, projectArkModSettings, readArkModMapOverrides, readArkModScopes, type ArkModChange, type ArkModScope } from "./ark-map-mods";
import "./ark-mod-maps.css";

// Editor snapshots carry this read-only baseline through their existing draft lifecycle.
// Only the current Mod/file delta leaves this component; this key is never persisted.
const EDITOR_BASE = "__ark_mod_editor_baseline";

function MapMultiSelect(props: {
  kind: "membership" | "configuration"; label: string; maps: readonly ArkModScope[]; selected: readonly string[];
  disabled: boolean; locale: string; onChange: (selected: string[]) => void;
  sharedChoice?: { selected: boolean; label: string; onChange: (selected: boolean) => void };
  action?: { label: string; disabled: boolean; onClick: () => void };
}) {
  const id = useId(), root = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null), focusOnOpen = useRef(false);
  const [open, setOpen] = useState(false), [placement, setPlacement] = useState({ above: false, height: 240 });
  const names = props.maps.filter((map) => props.selected.includes(map.key)).map((map) => map.name);
  const selectedNames = names.join(props.locale === "zh-CN" ? "、" : ", ");
  const shared = props.sharedChoice?.selected === true;
  const summary = props.sharedChoice?.selected ? props.sharedChoice.label : names.length === 0 ? props.locale === "zh-CN" ? "未选择地图" : "No maps selected"
    : names.length === props.maps.length ? props.locale === "zh-CN" ? "全部地图" : "All maps" : selectedNames;
  const count = `${names.length}/${props.maps.length}`;

  function show(focusFirst = false) {
    const bounds = trigger.current?.getBoundingClientRect();
    if (!bounds) return;
    const panel = root.current?.closest(".mw-detail-config-panel")?.getBoundingClientRect();
    const below = Math.min(panel?.bottom ?? innerHeight, innerHeight) - bounds.bottom - 12;
    const above = bounds.top - Math.max(panel?.top ?? 0, 0) - 12;
    const preferred = Math.min(240, (props.maps.length + (props.sharedChoice ? 1 : 0) + (props.action ? 1 : 0)) * 38 + 24);
    const upwards = below < preferred && above > below;
    setPlacement({ above: upwards, height: Math.max(40, Math.min(240, upwards ? above : below)) });
    focusOnOpen.current = focusFirst;
    setOpen(true);
    if (open && focusFirst) menu.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus();
  }
  useEffect(() => {
    if (!open) return;
    if (focusOnOpen.current) {
      focusOnOpen.current = false;
      menu.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus();
    }
    const outside = (event: Event) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    const dismiss = () => setOpen(false);
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
    };
  }, [open]);
  return <div className="ark-mod-map-select" ref={root} data-open={open || undefined}
    onKeyDown={(event) => {
      if (event.key === "Escape" && open) {
        event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus();
      } else if (event.key === "ArrowDown" && event.target === trigger.current) {
        event.preventDefault(); show(true);
      }
    }}>
    <button ref={trigger} type="button" className="settings-schema-input ark-mod-map-select__trigger"
      data-ark-mod-map-select={props.kind} disabled={props.disabled} aria-expanded={open} aria-controls={id}
      aria-label={`${props.label}: ${summary}${shared ? "" : ` (${count})`}`} title={shared ? summary : selectedNames || summary}
      onClick={() => open ? setOpen(false) : show()}>
      <span className="ark-mod-map-select__summary">{summary}</span>
      {!shared ? <span className="ark-mod-map-select__count">{count}</span> : null}
      <span className="ark-mod-map-select__arrow" aria-hidden="true" />
    </button>
    {open ? <div ref={menu} id={id} className="ark-mod-map-select__menu" role="group" aria-label={props.label}
      data-ark-mod-map-menu={props.kind} data-above={placement.above || undefined} style={{ maxHeight: placement.height }}>
      {props.sharedChoice ? <>
        <label className="ark-mod-map-select__option">
          <input type="checkbox" disabled={props.disabled} checked={shared}
            data-ark-mod-membership-shared={props.kind === "membership" ? "" : undefined}
            data-ark-mod-configuration-shared={props.kind === "configuration" ? "" : undefined}
            onChange={(event) => props.sharedChoice?.onChange(event.target.checked)} />
          <span>{props.sharedChoice.label}</span>
        </label>
        <div className="ark-mod-map-select__divider" role="separator" />
      </> : null}
      {props.maps.map((map) => <label key={map.key} className="ark-mod-map-select__option">
        <input type="checkbox" disabled={props.disabled} checked={props.selected.includes(map.key)}
          data-ark-mod-target={props.kind === "membership" ? map.key : undefined}
          data-ark-mod-configuration-target={props.kind === "configuration" ? map.key : undefined}
          onChange={(event) => props.onChange(event.target.checked ? [...props.selected, map.key] : props.selected.filter((key) => key !== map.key))} />
        <span>{map.name}</span>
        {!map.enabled ? <small>{props.locale === "zh-CN" ? "已停用" : "Disabled"}</small> : null}
      </label>)}
      {props.action ? <>
        <div className="ark-mod-map-select__divider" role="separator" />
        <button type="button" className="ark-mod-map-select__action" data-ark-mod-configuration-inherit
          disabled={props.disabled || props.action.disabled} onClick={props.action.onClick}>{props.action.label}</button>
      </> : null}
    </div> : null}
  </div>;
}

export function ArkModMapConfiguration(props: {
  instanceId: string; instanceName: string; moduleId: string; modId: string; settings: SettingsObject; disabled: boolean; readOnly: boolean; installed?: boolean;
  onChange: (change: ArkModChange, base: SettingsObject) => Promise<SettingsObject>;
  onReload: () => Promise<SettingsObject>;
}) {
  const { locale } = useI18n(), text = (zh: string, en: string) => locale === "zh-CN" ? zh : en;
  const maps = useMemo(() => readArkModScopes(props.settings, props.instanceName), [props.settings, props.instanceName]);
  const enabled = arkModEnabledMaps(props.settings, props.moduleId, props.modId);
  const inherited = !arkModHasMembershipOverrides(props.settings, props.moduleId, props.modId);
  const [scopes, setScopes] = useState<Record<string, "shared" | "maps">>({});
  const [targets, setTargets] = useState<Record<string, string[]>>({});
  const [references, setReferences] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { setError(null); }, [props.modId]);
  const scope = scopes[props.modId] ?? "shared";
  const selected = (targets[props.modId] ?? enabled).filter((key) => maps.some((map) => map.key === key));
  const reference = selected.includes(references[props.modId]) ? references[props.modId] : selected[0];
  const referenceMap = maps.find((map) => map.key === reference);
  const disabled = props.disabled || props.readOnly;
  const effectiveConfig = (root: SettingsObject, map: string | null) => readAsaModConfigurations(projectArkModSettings(root, props.moduleId, map))[props.modId]
    ?? { game_ini: "", game_user_settings_ini: "" };
  const configurations = scope === "maps" ? selected.map((map) => effectiveConfig(props.settings, map)) : [];
  const mixed = configurations.some((config) => JSON.stringify(config) !== JSON.stringify(configurations[0]));
  const configurationOverrides = readArkModMapOverrides(props.settings, props.moduleId);
  const canResetConfiguration = selected.some((map) => configurationOverrides[map]?.mods[props.modId]?.configuration !== undefined);
  async function apply(change: ArkModChange) {
    setError(null);
    try { await props.onChange(change, props.settings); }
    catch (failure) { setError(describeError(failure)); }
  }
  function editorSettings(root: SettingsObject): SettingsObject {
    return { ...projectArkModSettings(root, props.moduleId, scope === "maps" ? reference ?? null : null), [EDITOR_BASE]: structuredClone(root) };
  }
  return <div className="ark-mod-maps" data-ark-current-mod={props.modId}>
    <fieldset className="ark-mod-maps__group" data-ark-mod-targets={props.modId} disabled={disabled}>
      <legend>{text("生效地图", "Enabled maps")}</legend>
      <MapMultiSelect key={`${props.modId}:membership`} kind="membership" label={text("生效地图", "Enabled maps")}
        maps={maps} selected={inherited ? [] : enabled} disabled={disabled} locale={locale}
        sharedChoice={{ selected: inherited, label: text("跟随共用", "Use shared defaults"),
          onChange: (shared) => {
            void apply(shared ? { kind: "inherit-membership", modId: props.modId }
              : { kind: "membership", modId: props.modId, maps: [] });
          } }}
        onChange={(maps) => { void apply({ kind: "membership", modId: props.modId, maps }); }} />
    </fieldset>
    {error ? <p className="ark-mod-maps__error" role="alert">{error}</p> : null}
    {props.moduleId === "arksurvivalascended" ? <>
      <fieldset className="ark-mod-maps__group" data-ark-mod-configuration-targets disabled={disabled}>
        <legend>{text("参数配置范围", "Configuration target")}</legend>
        <MapMultiSelect key={`${props.modId}:configuration`} kind="configuration"
          label={text("参数配置范围", "Configuration target")} maps={maps} selected={scope === "shared" ? [] : selected}
          disabled={disabled} locale={locale}
          sharedChoice={{ selected: scope === "shared", label: text("共用默认参数", "Shared configuration"),
            onChange: (shared) => {
              if (!shared) setTargets((previous) => ({ ...previous, [props.modId]: [] }));
              setScopes((previous) => ({ ...previous, [props.modId]: shared ? "shared" : "maps" }));
            } }}
          onChange={(maps) => {
            setTargets((previous) => ({ ...previous, [props.modId]: maps }));
            setScopes((previous) => ({ ...previous, [props.modId]: "maps" }));
          }}
          action={{ label: text("恢复共用参数", "Use shared configuration"),
            disabled: scope !== "maps" || !selected.length || !canResetConfiguration,
            onClick: () => { void apply({ kind: "inherit-configuration", modId: props.modId, maps: selected }); } }} />
        {scope === "maps" ? <>
          {mixed ? <div data-ark-mod-mixed className="ark-mod-maps__mixed" role="status">
            <p>{text("所选地图的参数不同。保存会同步这些地图中当前 MOD 的当前 INI 文件。", "Selected maps have different options. Saving synchronizes this Mod's current INI file on those maps.")}</p>
            <label>{text("编辑基准", "Configuration to display")}<select className="settings-schema-input" data-ark-mod-configuration-reference value={reference}
              onChange={(event) => setReferences((previous) => ({ ...previous, [props.modId]: event.target.value }))}>
              {selected.map((map) => <option key={map} value={map}>{maps.find((entry) => entry.key === map)?.name ?? map}</option>)}
            </select></label>
          </div> : null}
          {!selected.length ? <p role="status">{text("请选择要配置参数的地图。此选择不会改变 MOD 的生效地图。", "Select maps to configure. This selection does not change where the Mod is enabled.")}</p> : null}
        </> : null}
      </fieldset>
      <AsaModConfiguration instanceId={props.instanceId} modId={props.modId}
        scopeKey={scope === "shared" ? "shared" : `maps:${[...selected].sort().join(",")}:reference:${reference ?? ""}`}
        settings={editorSettings(props.settings)} disabled={props.disabled || (scope === "maps" && !selected.length)} readOnly={props.readOnly} installed={props.installed}
        onSave={async (next, expected, draftBase) => {
          const before = readAsaModConfigurations(expected)[props.modId], after = readAsaModConfigurations(next)[props.modId];
          const file = (Object.keys(ASA_MOD_INI_FILES) as AsaModIniFile[]).find((key) => (before?.[key] ?? "") !== (after?.[key] ?? ""));
          if (!file) return editorSettings(props.settings);
          const storedBase = (draftBase ?? expected)[EDITOR_BASE];
          if (!storedBase || typeof storedBase !== "object" || Array.isArray(storedBase)) throw new Error(text("参数基准无效，请重新读取已保存配置。", "The configuration baseline is invalid. Reload saved configuration."));
          const saved = await props.onChange({ kind: "configuration", modId: props.modId, maps: scope === "shared" ? null : selected,
            file, text: after?.[file] ?? "" }, storedBase as SettingsObject);
          return editorSettings(saved);
        }} onReload={async () => editorSettings(await props.onReload())} />
      {scope === "maps" && referenceMap ? <p className="ark-mod-maps__note">{text(`当前显示 ${referenceMap.name}；保存仅更新所选地图的此 MOD。`, `Displaying ${referenceMap.name}; saving updates only this Mod on selected maps.`)}</p> : null}
    </> : null}
  </div>;
}
