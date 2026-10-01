import type { ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";

const BOOLEAN_FIELDS = ["coop_quests", "easy_explore"] as const;
const NUMBER_FIELDS = [
  "mob_health_multiplier",
  "mob_damage_multiplier",
  "ship_health_multiplier",
  "ship_damage_multiplier",
  "boarding_difficulty_multiplier",
  "coop_stats_correction_modifier",
  "coop_ship_stats_correction_modifier"
] as const;
const NUMBER_RANGES: Record<(typeof NUMBER_FIELDS)[number], readonly [number, number]> = {
  mob_health_multiplier: [0.2, 5],
  mob_damage_multiplier: [0.2, 5],
  ship_health_multiplier: [0.4, 5],
  ship_damage_multiplier: [0.2, 2.5],
  boarding_difficulty_multiplier: [0.2, 5],
  coop_stats_correction_modifier: [0, 2],
  coop_ship_stats_correction_modifier: [0, 2]
};
const SOURCE_KEYS: Record<string, string> = {
  world_name: "WorldName",
  world_preset_type: "WorldPresetType",
  coop_quests: "WDS.Parameter.Coop.SharedQuests",
  easy_explore: "WDS.Parameter.EasyExplore",
  mob_health_multiplier: "WDS.Parameter.MobHealthMultiplier",
  mob_damage_multiplier: "WDS.Parameter.MobDamageMultiplier",
  ship_health_multiplier: "WDS.Parameter.ShipsHealthMultiplier",
  ship_damage_multiplier: "WDS.Parameter.ShipsDamageMultiplier",
  boarding_difficulty_multiplier: "WDS.Parameter.BoardingDifficultyMultiplier",
  coop_stats_correction_modifier: "WDS.Parameter.Coop.StatsCorrectionModifier",
  coop_ship_stats_correction_modifier: "WDS.Parameter.Coop.ShipStatsCorrectionModifier",
  combat_difficulty: "WDS.Parameter.CombatDifficulty"
};

function copyKey(fieldKey: string, suffix: "title" | "description") {
  return `settings.schema.windrose.${fieldKey}.${suffix}`;
}

function fieldInputId(fieldKey: string) {
  return buildConfigurationFieldIds(fieldKey, "configuration-windrose").inputId;
}

function playerDescription(value: string, fallback: string) {
  const description = value
    .replace(/^Written(?:\s+to|\s+as)?.*?\.(?=\s|$)\s*/u, "")
    .replace(/^Legacy\s+[^.]+\s+flag\.\s*/u, "")
    .replace(/^作为[^。]*写入[^。]*。\s*/u, "")
    .replace(/^原生键名[^；;。]*(?:[；;。]\s*)/u, "")
    .trim();
  return description || fallback;
}

function FieldFrame(props: {
  children: (descriptionId?: string) => ReactNode;
  description: string;
  fieldKey: string;
  t: TranslateFn;
  title: string;
}) {
  const inputId = fieldInputId(props.fieldKey);
  const help = useConfigurationFieldHelp(`${inputId}-description`, props.description, props.title, props.t);
  return (
    <div className="configuration-field settings-schema-field" data-field-key={props.fieldKey}
      data-windrose-native-key={SOURCE_KEYS[props.fieldKey]} ref={help.anchorRef} {...help.interactionProps}>
      <label className="detail-label settings-field-label" htmlFor={inputId}>{props.title}</label>
      {help.helpNode}
      <div className="configuration-field-control">{props.children(help.descriptionId)}</div>
    </div>
  );
}

function worldEditState(props: ConfigurationSpecializedRendererProps) {
  const selectedWorldId = typeof props.settings.world_island_id === "string"
    ? props.settings.world_island_id.trim()
    : "";
  const runtimeStatus = String(props.details.summary.status).trim().toLowerCase();
  const serverMustStop = ["starting", "running", "stopping"].includes(runtimeStatus);
  const disabled = props.disabled || serverMustStop || selectedWorldId.length === 0;
  return { selectedWorldId, serverMustStop, disabled };
}

export function WindroseWorldNameField(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const { disabled, selectedWorldId, serverMustStop } = worldEditState(props);
  const help = serverMustStop
    ? t("windrose.settings.worldParameters.stopRequired", undefined,
      "Stop this server before changing world settings. Saved changes take effect on the next start.")
    : !selectedWorldId
      ? t("windrose.settings.worldParameters.selectionRequired", undefined,
        "Choose an existing world in Room settings. These settings remain unavailable until one world can be identified.")
      : t("windrose.settings.worldParameters.worldNameHelp", undefined,
        "Sets the name shown to players for the selected world.");

  return (
    <div className="settings-schema-grid">
      <FieldFrame fieldKey="world_name" t={t}
        title={t(copyKey("world_name", "title"), undefined, "World Name")}
        description={help}>
        {(descriptionId) => <input id={fieldInputId("world_name")} className="settings-schema-input" type="text"
          value={String(props.settings.world_name ?? "")} disabled={disabled}
          aria-describedby={descriptionId}
          onChange={(event) => props.onPatch({ world_name: event.target.value })} />}
      </FieldFrame>
    </div>
  );
}

export function WindroseWorldSettingsPanel(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const { selectedWorldId, serverMustStop, disabled } = worldEditState(props);
  const title = (fieldKey: string, fallback: string) => t(copyKey(fieldKey, "title"), undefined, fallback);
  const description = (fieldKey: string, fallback: string) => playerDescription(
    t(copyKey(fieldKey, "description"), undefined, fallback),
    fallback
  );

  return (
    <section className="guided-field-group guided-field-group--windrose-world-parameters">
      <div className="guided-field-group-head">
        <div>
          <h5 className="guided-field-group-title">
            {t("windrose.settings.worldParameters.title", undefined, "Existing world parameters")}
          </h5>
        </div>
      </div>
      {serverMustStop ? (
        <div className="configuration-workspace__notice" role="status" data-windrose-world-state="server-stop-required">
          {t(
            "windrose.settings.worldParameters.stopRequired",
            undefined,
            "Stop this server before changing world settings. Saved changes take effect on the next start."
          )}
        </div>
      ) : !selectedWorldId ? (
        <div className="configuration-workspace__notice" role="status" data-windrose-world-state="selection-required">
          {t(
            "windrose.settings.worldParameters.selectionRequired",
            undefined,
            "Choose an existing world in Room settings. These settings remain unavailable until one world can be identified."
          )}
        </div>
      ) : (
        <div className="configuration-workspace__notice" role="status" data-windrose-world-state="selection-pending-verification">
          {t(
            "windrose.settings.worldParameters.selectionPendingVerification",
            { worldId: selectedWorldId },
            "Selected {worldId}. Saved changes take effect on the next server start."
          )}
        </div>
      )}
      <div className="settings-schema-grid">
        <FieldFrame fieldKey="world_preset_type" t={t} title={title("world_preset_type", "World Preset")}
          description={description("world_preset_type", "Custom world values make the game force Custom on the next launch.")}>
          {(descriptionId) => <select id={fieldInputId("world_preset_type")} className="settings-schema-input settings-schema-select"
            value={String(props.settings.world_preset_type ?? "Medium")} disabled={disabled}
            aria-describedby={descriptionId}
            onChange={(event) => props.onPatch({ world_preset_type: event.target.value })}>
            {[
              ["Easy", t("windrose.settings.difficulty.easy", undefined, "Easy")],
              ["Medium", t("windrose.settings.difficulty.medium", undefined, "Medium")],
              ["Hard", t("windrose.settings.difficulty.hard", undefined, "Hard")]
            ].map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>}
        </FieldFrame>
        {BOOLEAN_FIELDS.map((fieldKey) => (
          <FieldFrame key={fieldKey} fieldKey={fieldKey} t={t}
            title={title(fieldKey, fieldKey === "coop_quests" ? "Shared Co-op Quests" : "Immersive Exploration")}
            description={description(fieldKey, fieldKey === "coop_quests"
              ? "Shares completed co-op quests with players who have the same quest active."
              : "Hides map markers so players must explore without point-of-interest guidance.")}>
            {(descriptionId) => <input id={fieldInputId(fieldKey)} type="checkbox"
              checked={Boolean(props.settings[fieldKey])} disabled={disabled}
              aria-describedby={descriptionId}
              onChange={(event) => props.onPatch({ [fieldKey]: event.target.checked })} />}
          </FieldFrame>
        ))}
        {NUMBER_FIELDS.map((fieldKey) => {
          const [minimum, maximum] = NUMBER_RANGES[fieldKey];
          return (
            <FieldFrame key={fieldKey} fieldKey={fieldKey} t={t}
              title={title(fieldKey, fieldKey.replace(/_/g, " "))}
              description={description(fieldKey, `Official range: ${minimum} to ${maximum}.`)}>
              {(descriptionId) => <input id={fieldInputId(fieldKey)} className="settings-schema-input" type="number"
                min={minimum} max={maximum} step="any" value={String(props.settings[fieldKey] ?? "")}
                disabled={disabled} aria-describedby={descriptionId}
                onChange={(event) => props.onPatch({
                  [fieldKey]: event.target.value === "" ? "" : Number(event.target.value)
                })} />}
            </FieldFrame>
          );
        })}
        <FieldFrame fieldKey="combat_difficulty" t={t} title={title("combat_difficulty", "Combat Difficulty")}
          description={description("combat_difficulty", "Boss difficulty and general enemy aggression.")}>
          {(descriptionId) => <select id={fieldInputId("combat_difficulty")} className="settings-schema-input settings-schema-select"
            value={String(props.settings.combat_difficulty ?? "Normal")} disabled={disabled}
            aria-describedby={descriptionId}
            onChange={(event) => props.onPatch({ combat_difficulty: event.target.value })}>
            {[
              ["Easy", t("windrose.settings.difficulty.easy", undefined, "Easy")],
              ["Normal", t("windrose.settings.difficulty.normal", undefined, "Normal")],
              ["Hard", t("windrose.settings.difficulty.hard", undefined, "Hard")]
            ].map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>}
        </FieldFrame>
      </div>
    </section>
  );
}
