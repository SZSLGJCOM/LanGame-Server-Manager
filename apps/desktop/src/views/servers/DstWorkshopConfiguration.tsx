import { useId, useState } from "react";
import { useI18n } from "../../i18n";
import type { SettingsObject } from "../settings/settings-schema";
import { DONTSTARVE_SHARD_NAMES, getDontStarveLayoutShards, isDontStarveShardActive,
  type DontStarveShard } from "../settings/modules/dontstarve-shards";
import { DstModConfigPanel } from "./DstModConfigPanel";
import { useDstModConfigurationSpec } from "./useDstModConfigurationSpec";
import { hasDstRawModOverrides } from "./mod-workbench-dst-policy";

interface DstWorkshopConfigurationProps {
  instanceId: string;
  settings: SettingsObject;
  selectedModId?: string | null;
  disabled?: boolean;
  readOnly?: boolean;
  scanNonce: number;
  canInstallMissingMod?: boolean;
  installingMissingMod?: boolean;
  onInstallMissingMod?: (modId: string) => void;
  onSettingsChange?: (settings: SettingsObject) => void;
}

export function DstWorkshopConfiguration(props: DstWorkshopConfigurationProps) {
  const { t } = useI18n();
  const shardSelectId = useId();
  const [selectedShard, setShard] = useState<"all" | DontStarveShard>("all");
  const shards = getDontStarveLayoutShards(props.settings);
  const shard = selectedShard === "all" || shards.includes(selectedShard) ? selectedShard : "all";
  const cavesDisabled = shard === "caves" && !isDontStarveShardActive(props.settings, "caves");
  const configuration = useDstModConfigurationSpec(
    props.instanceId, props.selectedModId, props.scanNonce, !props.readOnly
  );
  const settings = shard === "all" ? props.settings : isolateShardSettings(props.settings, shard);

  function updateShardSettings(nextSettings: SettingsObject) {
    if (props.readOnly || !props.onSettingsChange || shard === "all" || cavesDisabled || props.disabled) return;
    const next = { ...props.settings };
    const field = `${shard}_mod_configuration_options`;
    if (Object.prototype.hasOwnProperty.call(nextSettings, "master_mod_configuration_options")) {
      next[field] = nextSettings.master_mod_configuration_options;
    } else {
      delete next[field];
    }
    props.onSettingsChange(next);
  }

  return (
    <>
      <label className="settings-schema-field dst-mod-shard-field" htmlFor={shardSelectId}>
        <span className="settings-field-label">
          {t("dst.settings.modConfiguration.shard", undefined, "Apply to")}
        </span>
        <select id={shardSelectId} className="settings-schema-input" value={shard}
          onChange={(event) => {
            const value = event.target.value;
            if (value === "all" || shards.includes(value as DontStarveShard)) setShard(value as "all" | DontStarveShard);
          }}>
          <option value="all">{t("dst.settings.modConfiguration.allShards", undefined, "All layout shards")}</option>
          {shards.map((key) => <option key={key} value={key}>
            {t(`dst.settings.modConfiguration.${key}.label`, undefined, DONTSTARVE_SHARD_NAMES[key])}
          </option>)}
        </select>
      </label>
      {cavesDisabled ? <div className="mw-selected-config-empty-note" role="status">
        {t("dst.settings.caves.inactive", undefined,
          "Caves are disabled. Their settings are preserved and will apply on the next server start after enabling Caves.")}
      </div> : null}
      <DstModConfigPanel
        settings={settings}
        selectedModId={props.selectedModId}
        configurationSpecs={configuration.specs}
        loadingSpecs={configuration.loading}
        configurationError={configuration.error}
        onRetryConfiguration={props.readOnly ? undefined : configuration.retry}
        disabled={props.disabled || cavesDisabled}
        readOnly={props.readOnly}
        canInstallMissingMod={props.canInstallMissingMod}
        installingMissingMod={props.installingMissingMod}
        onInstallMissingMod={props.onInstallMissingMod}
        onSettingsChange={props.readOnly ? undefined : shard === "all" ? props.onSettingsChange : updateShardSettings}
        compact
      />
      {hasDstRawModOverrides(settings) ? <details className="mw-selected-config-empty-note">
        <summary>{t("dst.settings.modConfiguration.rawSource", undefined, "Source modoverrides.lua")}</summary>
        <p>{t("dst.settings.modConfiguration.rawSourceBody", undefined,
          "The source Lua controls enablement and options. Literal declarations are shown read-only; nested options and scripts remain in the original file. Edit the complete file under Advanced.")}</p>
        {(shard === "all" ? shards : [shard]).map((key) => {
          const raw = props.settings[`${key}_modoverrides_lua`];
          return typeof raw === "string" && hasDstRawModOverrides({ master_modoverrides_lua: raw })
            ? <div key={key}><strong>{DONTSTARVE_SHARD_NAMES[key]}</strong>
              <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{raw}</pre></div> : null;
        })}
      </details> : null}
    </>
  );
}

function isolateShardSettings(settings: SettingsObject, shard: DontStarveShard): SettingsObject {
  const enabled = settings[`${shard}_enabled_workshop_mod_ids`];
  const configuration = settings[`${shard}_mod_configuration_options`];
  const raw = settings[`${shard}_modoverrides_lua`];
  return {
    master_enabled_workshop_mod_ids: enabled,
    caves_enabled_workshop_mod_ids: enabled,
    master_mod_configuration_options: configuration,
    caves_mod_configuration_options: configuration,
    master_modoverrides_lua: raw,
    caves_modoverrides_lua: raw
  };
}
