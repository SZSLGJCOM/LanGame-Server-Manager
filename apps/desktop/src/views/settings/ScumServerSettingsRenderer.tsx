import { useMemo, useState, type ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { scumNativeMessageKey } from "../../i18n/games/scum-native-messages";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import {
  isScumWipeKey,
  patchScumNativeValue,
  readScumNativeValue,
  scumGroupId,
  SCUM_NATIVE_SETTINGS,
  scumPresentationSectionId,
  scumSectionForField,
  scumVirtualFieldKey,
  type ScumNativeSetting
} from "./scum-server-settings-inventory";

const GROUP_TITLES: Readonly<Record<string, string>> = {
  identity: "Identity & browser",
  communication: "Communication",
  performance: "Performance & network",
  "retention-logging": "Retention & logging",
  "gameplay-access": "Gameplay access",
  "maintenance-risk": "High-risk maintenance",
  wildlife: "Wildlife & fishing",
  encounters: "NPCs & encounters",
  "time-weather": "Time & weather",
  "cargo-drops": "Cargo drops",
  bunkers: "Bunkers & keycards",
  "world-rules": "World rules",
  "building-raiding": "Base building & raiding",
  resources: "World resources",
  items: "Items & harvesting",
  squads: "Squads",
  skills: "Skills",
  quests: "Quests",
  diagnostics: "Diagnostics",
  "survival-features": "Survival features",
  prices: "Respawn prices",
  cooldowns: "Respawn cooldowns",
  "spawn-rules": "Respawn rules",
  energy: "Fuel & battery",
  lifecycle: "Vehicle lifecycle",
  "fleet-limits": "Fleet limits",
  pvp: "Player damage",
  decay: "Decay & lock protection",
  "npc-structures": "NPC & structure damage"
};

const RISK_COPY: Readonly<Record<string, string>> = {
  partial_wipe: "Enables a partial wipe on the next server start; confirm your backups before continuing.",
  gold_wipe: "Resets gold-related data on the next server start; confirm your backups before continuing.",
  full_wipe: "Deletes player progression and character attributes on the next server start.",
  master_server_is_local_test: "Enables local test behavior and should remain off for normal public servers."
};

function FieldFrame(props: {
  children: (descriptionId?: string) => ReactNode;
  description: string;
  inputId: string;
  setting: ScumNativeSetting;
  t: TranslateFn;
  title: string;
}) {
  const help = useConfigurationFieldHelp(`${props.inputId}-description`, props.description, props.title, props.t);
  return (
    <div className="configuration-field settings-schema-field" data-field-key={scumVirtualFieldKey(props.setting)}
      data-scum-native-key={props.setting.nativeKey} ref={help.anchorRef} {...help.interactionProps}>
      <label className="detail-label settings-field-label" htmlFor={props.inputId}>{props.title}</label>
      {help.helpNode}
      <div className="configuration-field-control">{props.children(help.descriptionId)}</div>
    </div>
  );
}

function groupTitle(t: TranslateFn, groupId: string): string {
  return t(`scum.settings.groups.${groupId}`, undefined, GROUP_TITLES[groupId] ?? groupId);
}

function riskCopy(t: TranslateFn, setting: ScumNativeSetting): string {
  const effect = t(`scum.settings.risk.${setting.key}`, undefined, RISK_COPY[setting.key] ?? setting.title);
  return isScumWipeKey(setting.key) ? `${effect} ${t("scum.maintenance.persistenceNotice")}` : effect;
}

function settingTitle(t: TranslateFn, setting: ScumNativeSetting): string {
  return t(scumNativeMessageKey(setting.key, "title"), undefined, setting.title);
}

function settingDescription(t: TranslateFn, setting: ScumNativeSetting): string {
  if (setting.risk) return riskCopy(t, setting);
  return t(
    scumNativeMessageKey(setting.key, "description"),
    undefined,
    ""
  );
}

function renderControl(
  setting: ScumNativeSetting,
  value: unknown,
  inputId: string,
  describedBy: string | undefined,
  disabled: boolean,
  onValue: (value: unknown) => void
) {
  if (setting.type === "boolean") {
    return <input id={inputId} type="checkbox" checked={Boolean(value)} disabled={disabled}
      aria-describedby={describedBy} onChange={(event) => onValue(event.target.checked)} />;
  }
  if (setting.type === "integer" || setting.type === "number") {
    return <input id={inputId} className="settings-schema-input" type="number"
      step={setting.type === "integer" ? 1 : "any"} min={setting.minimum} max={setting.maximum}
      value={typeof value === "number" ? value : ""} disabled={disabled} aria-describedby={describedBy}
      onChange={(event) => onValue(event.target.value === "" ? "" : Number(event.target.value))} />;
  }
  const multiline = /Description|WelcomeMessage|MessageOfTheDay/u.test(setting.nativeKey);
  if (multiline) {
    return <textarea id={inputId} className="settings-schema-input settings-schema-textarea"
      value={typeof value === "string" ? value : ""} disabled={disabled} aria-describedby={describedBy}
      onChange={(event) => onValue(event.target.value.replace(/[\r\n]+/gu, " "))} />;
  }
  return <input id={inputId} className="settings-schema-input"
    type={setting.nativeKey === "scum.ServerPassword" ? "password" : "text"}
    value={typeof value === "string" ? value : ""} disabled={disabled} aria-describedby={describedBy}
    onChange={(event) => onValue(event.target.value)} />;
}

export function ScumServerSettingsRenderer(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const section = scumSectionForField(props.fieldKey) ?? props.sectionId;
  const isRoom = props.sectionId === "room" || props.sectionId === "maintenance";
  const [filter, setFilter] = useState("");
  const [pendingConfirmationKey, setPendingConfirmationKey] = useState<string | null>(null);
  const runtimeStatus = String(props.details.summary.status).trim().toLowerCase();
  const serverMustStop = runtimeStatus !== "stopped" || props.details.active_run !== null
    || props.details.summary.active_process_count !== 0;
  const inventory = useMemo(() => SCUM_NATIVE_SETTINGS.filter(
    (setting) => scumPresentationSectionId(setting) === props.sectionId
  ), [props.sectionId]);
  const visible = useMemo(() => {
    const query = filter.trim().toLocaleLowerCase();
    const editable = inventory.filter((setting) => setting.presentation !== "generated");
    if (!query) return editable;
    return editable.filter((setting) =>
      `${settingTitle(t, setting)} ${setting.title} ${setting.nativeKey} ${groupTitle(t, scumGroupId(setting))}`
        .toLocaleLowerCase()
        .includes(query)
    );
  }, [filter, inventory, t]);
  if (!section) return null;

  const grouped = new Map<string, ScumNativeSetting[]>();
  for (const setting of visible) {
    const groupId = scumGroupId(setting);
    grouped.set(groupId, [...(grouped.get(groupId) ?? []), setting]);
  }

  function patch(setting: ScumNativeSetting, value: unknown) {
    props.onPatch(patchScumNativeValue(props.settings, setting, value));
  }

  function requestValue(setting: ScumNativeSetting, value: unknown) {
    if (setting.risk && value === true) {
      setPendingConfirmationKey(setting.key);
      return;
    }
    setPendingConfirmationKey(null);
    patch(setting, value);
  }

  function confirm(setting: ScumNativeSetting) {
    if (serverMustStop || props.disabled) {
      setPendingConfirmationKey(null);
      return;
    }
    patch(setting, true);
    setPendingConfirmationKey(null);
  }

  return (
    <section className="settings-editor-stack scum-server-settings-renderer" data-scum-section={props.sectionId}
      aria-label={props.sectionId === "maintenance" ? t("scum.maintenance.title")
        : ["room", "network", "access", "runtime"].includes(props.sectionId)
        ? t(`settings.sections.${props.sectionId}`, undefined, section)
        : t(`scum.settings.sections.${props.sectionId}`, undefined, section)}>
      {!isRoom ? <div className="panel-head panel-head--compact panel-head--spread">
        <label className="settings-schema-field">
          <span className="settings-field-label">{t("settings.configuration.workspace.search", undefined, "Search configuration")}</span>
          <input className="settings-schema-input" type="search" value={filter}
            placeholder={t("scum.settings.renderer.filterPlaceholder", undefined, "Filter SCUM settings")}
            onChange={(event) => setFilter(event.target.value)} />
        </label>
        <span className="page-chip">{t("scum.settings.renderer.settingCount", { count: visible.length }, `${visible.length} settings`)}</span>
      </div> : null}
      {serverMustStop && inventory.some((setting) => setting.risk) ? (
        <div className="configuration-workspace__notice" role="status">
          {t(
            "scum.settings.renderer.stopRiskChanges",
            undefined,
            "Stop this SCUM server before changing wipe or internal-test switches."
          )}
        </div>
      ) : null}
      {[...grouped.entries()].map(([groupId, settings]) => {
        const title = groupTitle(t, groupId);
        return (
          <section className={`guided-field-group guided-field-group--scum-${groupId}`} key={groupId}>
            {!isRoom ? <div className="guided-field-group-head"><div>
              <h5 className="guided-field-group-title">{title}</h5>
            </div></div> : null}
            <div className="settings-schema-grid">
              {settings.map((setting) => {
                const value = readScumNativeValue(props.settings, setting);
                const inputId = buildConfigurationFieldIds(
                  scumVirtualFieldKey(setting),
                  "configuration-scum"
                ).inputId;
                const riskDisabled = Boolean(setting.risk && serverMustStop);
                return <FieldFrame key={setting.nativeKey} inputId={inputId} setting={setting} t={t}
                  title={settingTitle(t, setting)} description={settingDescription(t, setting)}>
                  {(descriptionId) => <>
                    {renderControl(setting, value, inputId, descriptionId, props.disabled || riskDisabled,
                      (nextValue) => requestValue(setting, nextValue))}
                    {pendingConfirmationKey === setting.key ? (
                      <div className="configuration-workspace__notice scum-settings-confirmation" role="alert">
                        <strong>{riskCopy(t, setting)}</strong>
                        <div className="scum-settings-actions">
                          <button type="button" className="ghost-button danger scum-settings-action" disabled={props.disabled || serverMustStop}
                            onClick={() => confirm(setting)}>
                            {t("scum.settings.renderer.confirmEnable", undefined, "Confirm enable")}
                          </button>
                          <button type="button" className="secondary-button scum-settings-action"
                            onClick={() => setPendingConfirmationKey(null)}>
                            {t("scum.settings.renderer.cancel", undefined, "Cancel")}
                          </button>
                        </div>
                      </div>
                    ) : null}
                  </>}
                </FieldFrame>;
              })}
            </div>
          </section>
        );
      })}
    </section>
  );
}
