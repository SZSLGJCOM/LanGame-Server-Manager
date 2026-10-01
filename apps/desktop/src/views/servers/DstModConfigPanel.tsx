import { ActivityNotice } from "../../components/ActivityNotice";
import { useId, useMemo } from "react";
import { useI18n } from "../../i18n";
import type {
  DstModConfigOptionSpec,
  DstModConfigurationSpec,
  DstModPrimitiveValue
} from "../../types";
import { parseWorkshopIdList } from "../settings/guided-settings";
import type { SettingsObject } from "../settings/settings-schema";
import { getDontStarveLayoutShards, type DontStarveShard } from "../settings/modules/dontstarve-shards";
import { DstModOptionField } from "./DstModOptionField";
import { readDstRawModDeclarations } from "./mod-workbench-dst-raw";

const DEFAULT_DST_MODOVERRIDES_LUA = "return {\n}\n";

type PrimitiveOptionValue = string | number | boolean;
type ModConfigurationMap = Record<string, Record<string, PrimitiveOptionValue>>;
type DstSpecMap = Record<string, DstModConfigurationSpec>;

interface DstModConfigPanelProps {
  settings: SettingsObject;
  configurationSpecs: DstModConfigurationSpec[];
  loadingSpecs: boolean;
  configurationError?: string | null;
  onRetryConfiguration?: () => void;
  disabled?: boolean;
  readOnly?: boolean;
  selectedModId?: string | null;
  compact?: boolean;
  canInstallMissingMod?: boolean;
  installingMissingMod?: boolean;
  onInstallMissingMod?: (modId: string) => void;
  onSettingsChange?: (nextSettings: SettingsObject) => void;
}

export function DstModConfigPanel(props: DstModConfigPanelProps) {
  const { t } = useI18n();
  const panelId = useId();
  const canEdit = !props.readOnly && !props.disabled && !props.loadingSpecs && !props.configurationError &&
    typeof props.onSettingsChange === "function";
  const selectedModId = props.selectedModId?.trim() || null;
  const shards = getDontStarveLayoutShards(props.settings);
  const rawDeclarations = shards.map((shard) => readDstRawModDeclarations(props.settings[`${shard}_modoverrides_lua`]));
  const enabledModIds = new Set(shards.flatMap((shard, index) => [
    ...parseWorkshopIdList(props.settings[`${shard}_enabled_workshop_mod_ids`]),
    ...rawDeclarations[index].filter((mod) => mod.enabled).map((mod) => mod.id)]));
  const configurationMaps = shards.map((shard, index) => {
    const raw = props.settings[`${shard}_modoverrides_lua`];
    return isCustomOverride(typeof raw === "string" ? raw : "")
      ? Object.fromEntries(rawDeclarations[index].map((mod) => [mod.id, mod.options]))
      : parseModConfigurationMap(props.settings[`${shard}_mod_configuration_options`]);
  });
  const specMap = useMemo(
    () => Object.fromEntries(props.configurationSpecs.map((spec) => [spec.mod_id, spec])) as DstSpecMap,
    [props.configurationSpecs]
  );
  const rawOverrideActive = shards.some((shard) => {
    const raw = props.settings[`${shard}_modoverrides_lua`];
    return isCustomOverride(typeof raw === "string" ? raw : "");
  });
  const shardOptions = configurationMaps.map((map) => selectedModId ? map[selectedModId] ?? {} : {});
  // Single-shard editing projects that shard into both maps; otherwise Master
  // is the displayed and saved baseline promised by the synchronization notice.
  const selectedOptions = shardOptions[0];
  const hasConfigurationOverrides = shardOptions.some((options) => Object.keys(options).length > 0);
  const modSpec = selectedModId ? specMap[selectedModId] ?? null : null;
  const configurationsDiffer = shardOptions.slice(1).some((options) =>
    !configurationOptionsEqual(selectedOptions, options, modSpec?.options ?? []));
  const editable = canEdit && !rawOverrideActive && !modSpec?.client_only;
  const autoSpecs = modSpec?.options ?? [];
  const knownKeys = new Set(autoSpecs.map((option) => option.name));
  const unknownOptionCount = Object.keys(selectedOptions).filter((key) => !knownKeys.has(key)).length;
  const specStatusMessage = props.configurationError
    ? t("dst.settings.modStatus.specParseErrorWithMessage", { message: props.configurationError },
      "Could not read this Mod's configuration definition: {message}")
    : modSpec ? describeSpecStatus(modSpec, {
    specMissingMod: t(
      "dst.settings.modStatus.specMissingMod",
      undefined,
      "This Mod's local files were not found. Download them to read its options."
    ),
    specMissingInfo: t(
      "dst.settings.modStatus.specMissingInfo",
      undefined,
      "The downloaded Mod is missing modinfo.lua. Download it again to read its options."
    ),
    specNoOptions: t(
      "dst.settings.modStatus.specNoOptions",
      undefined,
      "No editable options were read from this downloaded version. Check each required Mod separately for its options."
    ),
    specParseError: (message?: string | null) => message
      ? t(
        "dst.settings.modStatus.specParseErrorWithMessage",
        { message },
        "Could not read this Mod's configuration definition: {message}"
      )
      : t("dst.settings.modStatus.specParseError", undefined, "Could not read this Mod's configuration definition."),
    specWarning: (message?: string | null) => message
      ? t(
        "dst.settings.modStatus.specWarningWithMessage",
        { message },
        "Some Mod options were loaded with a warning: {message}"
      )
      : t("dst.settings.modStatus.specWarning", undefined, "Some Mod options were loaded with a warning.")
  }) : null;

  const copy = {
    noSelection: t("dst.settings.modStatus.noSelection", undefined, "Select a Mod from the list."),
    noOptions: t("dst.settings.modStatus.noOptions", undefined, "No editable options were read from this downloaded version. Check each required Mod separately for its options."),
    loadingSpecs: t("dst.settings.modStatus.loadingSpecs", undefined, "Reading Mod options…"),
    notLoaded: t("dst.settings.modStatus.specNotLoaded", undefined, "This Mod's configuration has not been read yet."),
    retry: t("dst.settings.modStatus.retryConfiguration", undefined, "Read configuration again"),
    notEnabled: t(
      "dst.settings.modStatus.notEnabled",
      undefined,
      "This Mod is disabled. Its configuration is preserved and will apply when it is enabled again."
    ),
    rawOverrideWarning: t(
      "dst.settings.modStatus.rawOverrideWarning",
      undefined,
      "A manual configuration override is active. Clear it before changing these options."
    ),
    configurationMismatch: t(
      "dst.settings.modStatus.configurationMismatch",
      undefined,
      "Existing shard values differ. The overworld value is shown; the next change will synchronize this Mod configuration."
    ),
    clearMod: t("dst.settings.modStatus.clearMod", undefined, "Restore all defaults"),
    installMissingMod: t("dst.settings.modStatus.installMissingMod", undefined, "Download and read configuration"),
    installingMissingMod: t("dst.settings.modStatus.installingMissingMod", undefined, "Downloading…"),
    boolTrue: t("dst.settings.modStatus.boolTrue", undefined, "On"),
    boolFalse: t("dst.settings.modStatus.boolFalse", undefined, "Off"),
    defaultChoice: t("dst.settings.modStatus.defaultChoice", undefined, "Use Mod default"),
    currentValue: (value: PrimitiveOptionValue) => t(
      "dst.settings.modStatus.currentValue",
      { value: String(value) },
      "Current value: {value}"
    ),
    autoFields: (count: number) => t("dst.settings.modStatus.autoFields", { count }, "{count} options"),
    configBadge: (count: number) => t("dst.settings.modStatus.configBadge", { count }, "{count} overrides"),
    unknownOptions: (count: number) => t(
      "dst.settings.modStatus.unknownOptions",
      { count },
      "{count} saved option(s) are no longer offered by this Mod. They are preserved until defaults are restored."
    )
  };

  function updateModOptions(modId: string, nextOptions: Record<string, PrimitiveOptionValue>) {
    if (!editable || !props.onSettingsChange) {
      return;
    }

    const normalizedOptions = Object.fromEntries(
      Object.entries(nextOptions).filter(([key]) => key.trim().length > 0)
    ) as Record<string, PrimitiveOptionValue>;
    const nextSettings: SettingsObject = { ...props.settings };
    shards.forEach((shard, index) => assignConfigurationMap(nextSettings, `${shard}_mod_configuration_options`,
      updateConfigurationMap(configurationMaps[index], modId, normalizedOptions)));
    props.onSettingsChange(nextSettings);
  }

  function handleStructuredChoiceChange(option: DstModConfigOptionSpec, nextValue: DstModPrimitiveValue) {
    if (!selectedModId) {
      return;
    }
    const nextOptions = { ...selectedOptions };
    const primitive = primitiveFromSpecValue(nextValue);
    if (
      nextValue.kind === "default" ||
      (primitive !== null && specValueMatchesPrimitive(option.default_value, primitive))
    ) {
      delete nextOptions[option.name];
    } else if (primitive !== null) {
      nextOptions[option.name] = primitive;
    }
    updateModOptions(selectedModId, nextOptions);
  }

  function handleStructuredTextChange(option: DstModConfigOptionSpec, nextValue: string) {
    if (!selectedModId) {
      return;
    }
    const nextOptions = { ...selectedOptions };
    if (nextValue.length === 0 || specValueMatchesPrimitive(option.default_value, nextValue)) {
      delete nextOptions[option.name];
    } else {
      nextOptions[option.name] = nextValue;
    }
    updateModOptions(selectedModId, nextOptions);
  }

  function handleStructuredNumberChange(option: DstModConfigOptionSpec, rawValue: string) {
    if (!selectedModId) {
      return;
    }
    const nextOptions = { ...selectedOptions };
    if (rawValue.trim().length === 0) {
      delete nextOptions[option.name];
      updateModOptions(selectedModId, nextOptions);
      return;
    }
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) {
      return;
    }
    if (specValueMatchesPrimitive(option.default_value, parsed)) {
      delete nextOptions[option.name];
    } else {
      nextOptions[option.name] = parsed;
    }
    updateModOptions(selectedModId, nextOptions);
  }

  const noOptions = !props.loadingSpecs && !props.configurationError && modSpec && autoSpecs.length === 0
    && (!specStatusMessage || modSpec.status === "no_options");
  const shellClass = `dst-mod-config-shell${props.compact ? " dst-mod-config-shell--compact" : ""}${!selectedModId || noOptions ? " dst-mod-config-shell--empty" : ""}`;
  if (!selectedModId) {
    return (
      <section className={shellClass}>
        <div className="mw-empty"><span>{copy.noSelection}</span></div>
      </section>
    );
  }

  return (
    <section className={shellClass} aria-busy={props.loadingSpecs}>
      {rawOverrideActive && canEdit ? <ActivityNotice tone="warning">{copy.rawOverrideWarning}</ActivityNotice> : null}
      {!props.readOnly && configurationsDiffer ? <ActivityNotice tone="warning">{copy.configurationMismatch}</ActivityNotice> : null}
      {!rawOverrideActive && !enabledModIds.has(selectedModId) ? <ActivityNotice>{copy.notEnabled}</ActivityNotice> : null}
      {props.loadingSpecs ? <ActivityNotice>{copy.loadingSpecs}</ActivityNotice> : null}
      {!props.loadingSpecs && modSpec?.client_only ? <ActivityNotice tone="warning">
        {t("dst.settings.modStatus.clientOnly", undefined, "This Mod only runs in the game client. Configure it in Don't Starve Together; server settings do not apply to it.")}
      </ActivityNotice> : null}
      {!props.readOnly && !props.loadingSpecs && !noOptions && (specStatusMessage || !modSpec) ? (
        <ActivityNotice tone={props.configurationError || modSpec?.status === "parse_error" ? "error"
          : modSpec?.status === "loaded_with_warnings" ? "warning" : "info"} action={<>
          {props.onRetryConfiguration && (props.configurationError || !modSpec ||
            modSpec.status === "parse_error" || modSpec.status === "missing_modinfo" ||
            modSpec.status === "no_options") ? (
            <button type="button" className="secondary-button" onClick={props.onRetryConfiguration}>
              {copy.retry}
            </button>
          ) : null}
          {modSpec?.status === "missing_mod" && props.onInstallMissingMod ? (
            <button
              type="button"
              className="secondary-button"
              disabled={props.disabled || props.installingMissingMod || !props.canInstallMissingMod}
              onClick={() => props.onInstallMissingMod?.(selectedModId)}
            >
              {props.installingMissingMod ? copy.installingMissingMod : copy.installMissingMod}
            </button>
          ) : null}
        </>}>{specStatusMessage ?? copy.notLoaded}</ActivityNotice>
      ) : null}

      {props.readOnly ? <section className="dst-mod-spec-section">
        <div className="dst-mod-spec-list">
          {Object.entries(selectedOptions).map(([key, value], index) => <div key={key} className="dst-mod-spec-row">
            <label className="dst-mod-spec-copy" htmlFor={`${panelId}-saved-option-${index}`}>
              <span className="dst-mod-spec-label">{key}</span>
            </label>
            <input id={`${panelId}-saved-option-${index}`} className="settings-schema-input" value={String(value)} readOnly />
          </div>)}
          {Object.keys(selectedOptions).length === 0 ? <p className="form-note" role="status">
            {t("servers.archives.configuration.notSaved")}</p> : null}
        </div>
      </section> : !props.loadingSpecs && !props.configurationError && autoSpecs.length > 0 ? (
        <section className="dst-mod-spec-section">
          <div className="dst-mod-section-head">
            <div className="dst-mod-config-counts">
              <span>{copy.autoFields(autoSpecs.length)}</span>
              <span>{copy.configBadge(Object.keys(selectedOptions).length)}</span>
            </div>
            <button
              type="button"
              className="ghost-button"
              disabled={!editable || !hasConfigurationOverrides}
              onClick={() => updateModOptions(selectedModId, {})}
            >
              {copy.clearMod}
            </button>
          </div>

          <div className="dst-mod-spec-list">
            {autoSpecs.map((option, index) => {
              const explicitValue = selectedOptions[option.name];
              const hasExplicit = Object.prototype.hasOwnProperty.call(selectedOptions, option.name);
              return (
                <DstModOptionField
                  key={`${selectedModId}:${option.name}`}
                  idPrefix={`dst-mod-option-${panelId}-${selectedModId}-${index}`}
                  option={option}
                  t={t}
                  explicitValue={explicitValue}
                  hasExplicit={hasExplicit}
                  editable={editable}
                  defaultChoice={copy.defaultChoice}
                  boolTrue={copy.boolTrue}
                  boolFalse={copy.boolFalse}
                  currentValue={copy.currentValue}
                  onChoiceChange={(value) => handleStructuredChoiceChange(option, value)}
                  onNumberChange={(value) => handleStructuredNumberChange(option, value)}
                  onTextChange={(value) => handleStructuredTextChange(option, value)}
                />
              );
            })}
          </div>
        </section>
      ) : noOptions ? (
        <div className="mw-empty">
          <span role="status">{specStatusMessage ?? copy.noOptions}</span>
          {modSpec.status === "no_options" && props.onRetryConfiguration ? <button type="button" className="secondary-button" onClick={props.onRetryConfiguration}>
            {copy.retry}
          </button> : null}
        </div>
      ) : null}

      {!props.loadingSpecs && !props.configurationError &&
        (autoSpecs.length > 0 || modSpec?.status === "no_options") && unknownOptionCount > 0 ? (
        <ActivityNotice>{copy.unknownOptions(unknownOptionCount)}</ActivityNotice>
      ) : null}
    </section>
  );
}

function parseModConfigurationMap(raw: unknown): ModConfigurationMap {
  if (!isRecord(raw)) {
    return {};
  }
  const next: ModConfigurationMap = {};
  for (const [rawModId, rawOptions] of Object.entries(raw)) {
    const normalizedModId = normalizeWorkshopId(rawModId) ?? rawModId.trim();
    if (!normalizedModId || !isRecord(rawOptions)) {
      continue;
    }
    const options: Record<string, PrimitiveOptionValue> = {};
    for (const [key, value] of Object.entries(rawOptions)) {
      if (key.trim() && isPrimitiveOptionValue(value)) {
        options[key] = value;
      }
    }
    if (Object.keys(options).length > 0) {
      next[normalizedModId] = options;
    }
  }
  return next;
}

function updateConfigurationMap(
  configurationMap: ModConfigurationMap,
  modId: string,
  options: Record<string, PrimitiveOptionValue>
): ModConfigurationMap {
  const nextMap = { ...configurationMap };
  if (Object.keys(options).length === 0) {
    delete nextMap[modId];
  } else {
    nextMap[modId] = options;
  }
  return nextMap;
}

function assignConfigurationMap(
  settings: SettingsObject,
  settingKey: `${DontStarveShard}_mod_configuration_options`,
  configurationMap: ModConfigurationMap
) {
  if (Object.keys(configurationMap).length === 0) {
    delete settings[settingKey];
  } else {
    settings[settingKey] = configurationMap;
  }
}

function configurationOptionsEqual(
  left: Record<string, PrimitiveOptionValue>,
  right: Record<string, PrimitiveOptionValue>,
  options: DstModConfigOptionSpec[]
): boolean {
  const defaults = new Map(options.map((option) => [option.name,
    option.default_value ? primitiveFromSpecValue(option.default_value) : null]));
  return Array.from(new Set([...Object.keys(left), ...Object.keys(right)])).every((key) => {
    const leftValue = Object.prototype.hasOwnProperty.call(left, key) ? left[key] : defaults.get(key);
    const rightValue = Object.prototype.hasOwnProperty.call(right, key) ? right[key] : defaults.get(key);
    return Object.is(leftValue, rightValue);
  });
}

function describeSpecStatus(
  spec: DstModConfigurationSpec,
  copy: {
    specMissingMod: string;
    specMissingInfo: string;
    specNoOptions: string;
    specParseError: (message?: string | null) => string;
    specWarning: (message?: string | null) => string;
  }
): string | null {
  switch (spec.status) {
    case "missing_mod":
      return copy.specMissingMod;
    case "missing_modinfo":
      return copy.specMissingInfo;
    case "no_options":
      return copy.specNoOptions;
    case "parse_error":
      return copy.specParseError(spec.message);
    case "loaded_with_warnings":
      return copy.specWarning(spec.message);
    default:
      return null;
  }
}

function primitiveFromSpecValue(value: DstModPrimitiveValue): PrimitiveOptionValue | null {
  switch (value.kind) {
    case "boolean":
    case "number":
    case "string":
      return value.value;
    case "default":
    default:
      return null;
  }
}

function specValueMatchesPrimitive(
  value: DstModPrimitiveValue | null | undefined,
  primitive: PrimitiveOptionValue
): boolean {
  if (!value) {
    return false;
  }
  switch (value.kind) {
    case "boolean":
    case "string":
      return value.value === primitive;
    case "number":
      return typeof primitive === "number" && Number(value.value) === primitive;
    case "default":
    default:
      return false;
  }
}

function isCustomOverride(rawOverride: string): boolean {
  const normalized = ensureTrailingNewline(rawOverride).trim();
  return normalized.length > 0 && normalized !== DEFAULT_DST_MODOVERRIDES_LUA.trim();
}

function ensureTrailingNewline(value: string): string {
  return value.endsWith("\n") ? value : `${value}\n`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isPrimitiveOptionValue(value: unknown): value is PrimitiveOptionValue {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean";
}

function normalizeWorkshopId(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const normalized = trimmed.startsWith("workshop-") ? trimmed.slice("workshop-".length) : trimmed;
  const match = normalized.match(/\d{6,}/);
  return match ? match[0] : null;
}
