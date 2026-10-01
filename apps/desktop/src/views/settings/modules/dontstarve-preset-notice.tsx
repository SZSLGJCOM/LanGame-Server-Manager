import { useI18n, type TranslateFn } from "../../../i18n";
import type { ConfigurationSpecializedRendererProps } from "../module-types";
import type { SettingsObject } from "../settings-schema";
import { isCustomRawValue } from "./dontstarve-validation";
import { getDontStarveWorldScriptMode } from "./dontstarve-world-lua";
import { isDontStarveShardActive } from "./dontstarve-shards";

export function DontStarveCavesNotice(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  if (isDontStarveShardActive(props.settings, "caves")) return null;
  return <div className="configuration-workspace__notice configuration-workspace__setting-notice" role="status">
    <span>{t("dst.settings.caves.inactive", undefined,
      "Caves are disabled. Their settings are preserved and will apply on the next server start after enabling Caves.")}</span>
    <button type="button" className="secondary-button" disabled={props.disabled}
      onClick={() => props.onPatch({ enable_caves: true })}>
      {t("dst.settings.caves.enable", undefined, "Enable Caves")}
    </button>
  </div>;
}

export function DontStarvePresetNotice(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  return <>
    {props.sectionId.endsWith("gen") ? <p className="configuration-workspace__notice" role="note">
      {t("dst.settings.generation.appliesToNew", undefined,
        "Set map size, terrain and initial resources before the first start. These options apply to newly generated worlds. Saving and restarting will not rebuild an existing map; use a new instance to keep the current world.")}
    </p> : null}
    <DontStarvePresetDetails {...props} />
  </>;
}

function DontStarvePresetDetails(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const shard = props.sectionId.startsWith("master") ? "master" : "caves";
  if (shard === "caves" && !isDontStarveShardActive(props.settings, "caves")) {
    return <DontStarveCavesNotice {...props} />;
  }
  if (getDontStarveWorldScriptMode(props.settings, shard)) {
    return <div className="configuration-workspace__notice configuration-workspace__setting-notice" role="status">
      <span>{t("dst.settings.worldScript.active", undefined,
        "This shard uses a Lua script that cannot be synchronized with guided controls. Edit its active source under Advanced; other settings remain editable.")}</span>
      {props.onNavigateField ? <button type="button" className="secondary-button" disabled={props.disabled}
        onClick={() => props.onNavigateField?.(isCustomRawValue(props.settings, `${shard}_worldgenoverride_lua`)
          ? `${shard}_worldgenoverride_lua` : `${shard}_world_overrides_extra`)}>
        {t("dst.settings.worldScript.edit", undefined, "Edit Lua")}
      </button> : null}
    </div>;
  }
  const notice = getDontStarvePresetNotice(props.settings, props.sectionId, t);
  return notice ? <p className="body-copy">{notice}</p> : null;
}

export function getDontStarvePresetNotice(
  settings: Readonly<SettingsObject>,
  sectionId: string,
  t: TranslateFn
): string | null {
  const shard = sectionId.startsWith("master") ? "master" : "caves";
  if (getDontStarveWorldScriptMode(settings, shard)) return null;
  if (isCustomRawValue(settings, `${shard}_worldgenoverride_lua`)) {
    return t("dst.settings.worldData.inheritance", undefined,
      "Options absent from the Lua configuration inherit the selected presets; the form initially shows base defaults. Adjusting a control updates the Lua configuration directly.");
  }
  const defaultPreset = shard === "master" ? "SURVIVAL_TOGETHER" : "DST_CAVE";
  const settingsPreset = String(settings[`${shard}_settings_preset`] ?? defaultPreset);
  const worldgenPreset = String(settings[`${shard}_worldgen_preset`] ?? defaultPreset);
  if (settingsPreset === defaultPreset && worldgenPreset === defaultPreset) return null;

  const inheritance = t(
    "dst.settings.presets.inheritance",
    { settingsPreset, worldgenPreset, defaultPreset, shard: shard === "master" ? "Master" : "Caves" },
    "{shard}: rules use {settingsPreset}; generation uses {worldgenPreset}. Unchanged options inherit these presets while the form shows base defaults from {defaultPreset}. Adjusting a control applies that value, including a base default."
  );
  return inheritance + (shard === "master" ? ` ${t(
    "dst.settings.presets.gameMode",
    undefined,
    "Set the cluster game mode to Survival. For a complete playstyle preset, choose the same ID under both World settings and World generation. Master supplies shared playstyle rules to Caves."
  )}` : "");
}
