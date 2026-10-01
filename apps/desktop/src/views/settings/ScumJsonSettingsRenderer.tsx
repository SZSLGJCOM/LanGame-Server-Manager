import { useId, useMemo, useState, type ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { scumJsonMessageKey } from "../../i18n/games/scum-native-messages";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import type { SettingsObject } from "./settings-schema";

type JsonRecord = Record<string, unknown>;

const TRADEABLE_FIELDS = [
  "tradeable-code",
  "base-purchase-price",
  "base-sell-price",
  "delta-price",
  "can-be-purchased",
  "required-famepoints",
  "available-after-sale-only"
] as const;
const RAID_FIELDS = ["day", "time", "start-announcement-time", "end-announcement-time"] as const;
const NOTIFICATION_FIELDS = ["day", "time", "duration", "color", "wait", "message"] as const;

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function humanize(value: string): string {
  return value.split("-").map((word) => word ? word[0].toUpperCase() + word.slice(1) : word).join(" ");
}

function fieldCopy(t: TranslateFn, key: string): { label: string; description: string } {
  const fallbackLabel = humanize(key);
  return {
    label: t(scumJsonMessageKey(key, "title"), undefined, fallbackLabel),
    description: t(
      scumJsonMessageKey(key, "description"),
      undefined,
      ""
    )
  };
}

function JsonField(props: {
  children: (descriptionId?: string) => ReactNode;
  description: string;
  helpId: string;
  label: string;
  nativeKey: string;
  t: TranslateFn;
}) {
  const help = useConfigurationFieldHelp(props.helpId, props.description, props.label, props.t);
  return (
    <label className="settings-schema-field" data-scum-native-key={props.nativeKey}
      ref={help.anchorRef} {...help.interactionProps}>
      <span className="settings-field-label">{props.label}</span>
      {help.helpNode}
      {props.children(help.descriptionId)}
    </label>
  );
}

function patchArrayItem(items: unknown[], index: number, key: string, value: unknown): unknown[] {
  return items.map((item, candidate) => candidate === index
    ? { ...(isRecord(item) ? item : {}), [key]: value }
    : item);
}

function structuredRowValue(
  surface: "raid_times" | "notifications",
  key: string,
  value: string
): string | string[] {
  if (surface !== "notifications" || key !== "time" || !value.includes(",")) return value;
  return value.split(",").map((entry) => entry.trim()).filter(Boolean);
}

function structuredSurfaceTitle(t: TranslateFn, fieldKey: "raid_times" | "notifications"): string {
  return fieldKey === "raid_times"
    ? t("scum.settings.sections.raid", undefined, "Raid times")
    : t("scum.settings.sections.notifications", undefined, "Notifications");
}

function EconomyEditor(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const economy = isRecord(props.settings.economy_override) ? props.settings.economy_override : {};
  const traders = isRecord(economy.traders) ? economy.traders : {};
  const globalEntries = Object.entries(economy).filter(([key]) => key !== "traders");
  const traderCodes = Object.keys(traders);
  const [selectedTrader, setSelectedTrader] = useState(traderCodes[0] ?? "A_0_Armory");
  const selectedRows = traders[selectedTrader];
  const rows: unknown[] = Array.isArray(selectedRows) ? selectedRows : [];

  function patchEconomy(next: JsonRecord) {
    props.onPatch({ economy_override: next });
  }

  function patchGlobal(key: string, value: string) {
    patchEconomy({ ...economy, [key]: value });
  }

  function patchRows(nextRows: unknown[]) {
    patchEconomy({ ...economy, traders: { ...traders, [selectedTrader]: nextRows } });
  }

  return (
    <section className="settings-editor-stack scum-json-settings-renderer" data-scum-json-surface="economy"
      aria-label={t("scum.settings.json.economyTitle", undefined, "Economy & traders")}>
      <section className="guided-field-group">
        <div className="guided-field-group-head"><h5 className="guided-field-group-title">
          {t("scum.settings.json.globalEconomy", undefined, "Global economy")}
        </h5></div>
        <div className="settings-schema-grid">
          {globalEntries.map(([key, value]) => {
            const copy = fieldCopy(t, key);
            const description = t(
              "scum.settings.json.globalHelp",
              { field: copy.label },
              copy.description
            );
            return <JsonField key={key} nativeKey={key} label={copy.label} t={t}
              helpId={`configuration-scum-economy-${key}-description`}
              description={description}>
              {(descriptionId) => <input className="settings-schema-input" type="text"
                value={String(value ?? "")} disabled={props.disabled} aria-describedby={descriptionId}
                onChange={(event) => patchGlobal(key, event.target.value)} />}
            </JsonField>;
          })}
        </div>
      </section>
      <section className="guided-field-group">
        <div className="guided-field-group-head"><h5 className="guided-field-group-title">
          {t("scum.settings.json.traderOverrides", undefined, "Trader inventory overrides")}
        </h5></div>
        <JsonField nativeKey="trader" label={fieldCopy(t, "trader").label} t={t}
          helpId="configuration-scum-economy-trader-description"
          description={t(
            "scum.settings.json.traderHelp",
            undefined,
            "Selects the trader whose inventory rules you want to edit."
          )}>
          {(descriptionId) => <select className="settings-schema-input settings-schema-select" value={selectedTrader}
            disabled={props.disabled} aria-describedby={descriptionId}
            onChange={(event) => setSelectedTrader(event.target.value)}>
            {traderCodes.map((code) => <option value={code} key={code}>{code}</option>)}
          </select>}
        </JsonField>
        <div className="configuration-workspace__structured-list">
          {rows.map((row, index) => {
            const record = isRecord(row) ? row : {};
            return <article className="runtime-box" key={`${selectedTrader}-${index}`}
              aria-label={t("scum.settings.json.row", { row: index + 1 }, `Row ${index + 1}`)}>
              <div className="panel-head panel-head--compact panel-head--spread"><strong>
                {t("scum.settings.json.row", { row: index + 1 }, `Row ${index + 1}`)}
              </strong>
                <button className="secondary-button scum-settings-action" type="button" disabled={props.disabled}
                  onClick={() => patchRows(rows.filter((_, candidate) => candidate !== index))}>
                  {t("scum.settings.json.remove", undefined, "Remove")}
                </button></div>
              <div className="settings-schema-grid">
                {TRADEABLE_FIELDS.map((key) => {
                  const copy = fieldCopy(t, key);
                  return <JsonField key={key} nativeKey={key} label={copy.label} t={t}
                    helpId={`configuration-scum-economy-${selectedTrader}-${index}-${key}-description`}
                    description={t(
                      "scum.settings.json.tradeableHelp",
                      { field: copy.label },
                      copy.description
                    )}>
                    {(descriptionId) => <input className="settings-schema-input" type="text"
                      value={String(record[key] ?? "")} disabled={props.disabled}
                      aria-describedby={descriptionId}
                      onChange={(event) => patchRows(patchArrayItem(rows, index, key, event.target.value))} />}
                  </JsonField>;
                })}
              </div>
            </article>;
          })}
        </div>
        <button className="secondary-button scum-settings-action" type="button" disabled={props.disabled}
          onClick={() => patchRows([
            ...rows,
            Object.fromEntries(TRADEABLE_FIELDS.map((key) => [key, ""]))
          ])}>
          {t("scum.settings.json.addTradeableRow", undefined, "Add tradeable row")}
        </button>
      </section>
    </section>
  );
}

function StructuredArrayEditor(props: ConfigurationSpecializedRendererProps & {
  fieldKey: "raid_times" | "notifications";
  title: string;
  fields: readonly string[];
}) {
  const { t } = useI18n();
  const surfaceValue = props.settings[props.fieldKey];
  const items: unknown[] = Array.isArray(surfaceValue) ? surfaceValue : [];
  const empty = Object.fromEntries(props.fields.map((key) => [key, ""]));
  const maximumReached = props.fieldKey === "raid_times" && items.length >= 50;
  const help = useConfigurationFieldHelp(useId(), maximumReached
    ? t("scum.settings.validation.raidMaximum", undefined, "SCUM allows at most 50 raid-time entries.")
    : undefined, undefined, undefined, "instructions");
  function patch(next: unknown[]) {
    props.onPatch({ [props.fieldKey]: next });
  }
  return (
    <section className="settings-editor-stack scum-json-settings-renderer" data-scum-json-surface={props.fieldKey}
      aria-label={props.title}>
      <div className="panel-head panel-head--compact"><span className="page-chip">
         {t("scum.settings.json.rowCount", { count: items.length }, `${items.length} rows`)}
       </span></div>
      <div className="configuration-workspace__structured-list">
        {items.map((item, index) => {
          const record = isRecord(item) ? item : {};
          return <article className="guided-field-group" key={index}
            aria-label={t("scum.settings.json.row", { row: index + 1 }, `Row ${index + 1}`)}>
            <div className="guided-field-group-head"><strong>
              {t("scum.settings.json.row", { row: index + 1 }, `Row ${index + 1}`)}
            </strong>
              <button className="secondary-button scum-settings-action" type="button" disabled={props.disabled}
                onClick={() => patch(items.filter((_, candidate) => candidate !== index))}>
                {t("scum.settings.json.remove", undefined, "Remove")}
              </button></div>
            <div className="settings-schema-grid">
              {props.fields.map((key) => {
                const copy = fieldCopy(t, key);
                const helpKey = props.fieldKey === "raid_times"
                  ? "scum.settings.json.raidHelp"
                  : "scum.settings.json.notificationHelp";
                return <JsonField key={key} nativeKey={key} label={copy.label} t={t}
                  helpId={`configuration-scum-${props.fieldKey}-${index}-${key}-description`}
                  description={t(helpKey, { field: copy.label }, copy.description)}>
                  {(descriptionId) => key === "message" ? <textarea
                    className="settings-schema-input settings-schema-textarea"
                    value={String(record[key] ?? "")} disabled={props.disabled}
                    aria-describedby={descriptionId}
                    onChange={(event) => patch(patchArrayItem(items, index, key, event.target.value))} />
                    : <input className="settings-schema-input" type="text"
                      value={Array.isArray(record[key]) ? record[key].join(",") : String(record[key] ?? "")}
                      disabled={props.disabled} aria-describedby={descriptionId}
                      onChange={(event) => patch(patchArrayItem(
                        items,
                        index,
                        key,
                        structuredRowValue(props.fieldKey, key, event.target.value)
                      ))} />}
                </JsonField>;
              })}
            </div>
          </article>;
        })}
      </div>
      <span className="scum-settings-action-help" ref={help.anchorRef} {...help.interactionProps}
        role={maximumReached ? "group" : undefined} tabIndex={maximumReached ? 0 : undefined}
        aria-label={maximumReached ? t("scum.settings.json.addRow", undefined, "Add row") : undefined}
        aria-describedby={help.descriptionId}>
        <button className="secondary-button scum-settings-action" type="button"
          disabled={props.disabled || maximumReached} aria-describedby={help.descriptionId}
          onClick={() => {
            if (props.fieldKey !== "raid_times" || items.length < 50) patch([...items, empty]);
          }}>{t("scum.settings.json.addRow", undefined, "Add row")}</button>
        {help.helpNode}
      </span>
    </section>
  );
}

export function ScumJsonSettingsRenderer(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const surface = props.fieldKey;
  const renderer = useMemo(() => {
    if (surface === "raid_times") return "raid";
    if (surface === "notifications") return "notifications";
    return "economy";
  }, [surface]);
  if (renderer === "economy") return <EconomyEditor {...props} />;
  if (renderer === "raid") {
    return <StructuredArrayEditor {...props} fieldKey="raid_times"
      title={t("scum.settings.json.raidTitle", undefined, "Global raid windows")}
      fields={RAID_FIELDS} />;
  }
  return <StructuredArrayEditor {...props} fieldKey="notifications"
    title={t("scum.settings.json.notificationsTitle", undefined, "Scheduled notifications")}
    fields={NOTIFICATION_FIELDS} />;
}

export function validateScumJsonSettings(
  settings: Readonly<SettingsObject>,
  t: TranslateFn = (key, _params, fallback) => fallback ?? key
) {
  const issues: Array<{ fieldKey: string; reason: string; message: string }> = [];
  if (!isRecord(settings.economy_override)) {
    issues.push({ fieldKey: "economy_override", reason: "type", message: t(
      "scum.settings.validation.economyObject", undefined, "Economy override must be a structured object."
    ) });
  } else if (Object.keys(settings.economy_override).length > 0 && !isRecord(settings.economy_override.traders)) {
    // The empty schema default selects the native economy defaults during rendering.
    issues.push({ fieldKey: "economy_override", reason: "traders", message: t(
      "scum.settings.validation.tradersObject", undefined, "Economy override must retain its traders object."
    ) });
  }
  for (const [fieldKey, required] of [["raid_times", RAID_FIELDS], ["notifications", NOTIFICATION_FIELDS]] as const) {
    const rows = settings[fieldKey];
    const fieldTitle = structuredSurfaceTitle(t, fieldKey);
    if (!Array.isArray(rows)) {
      issues.push({ fieldKey, reason: "type", message: t(
        "scum.settings.validation.structuredList", { field: fieldTitle }, `${fieldTitle} must be a structured row list.`
      ) });
      continue;
    }
    rows.forEach((row, index) => {
      const hasRequiredFields = isRecord(row) && required.every((key) => {
        const value = row[key];
        if (fieldKey === "notifications" && key === "time") {
          return (typeof value === "string" && value.trim().length > 0)
            || (Array.isArray(value) && value.length > 0
              && value.every((entry) => typeof entry === "string" && entry.trim().length > 0));
        }
        return typeof value === "string" && value.trim().length > 0;
      });
      if (!hasRequiredFields) {
        issues.push({ fieldKey, reason: "row", message: t(
          "scum.settings.validation.missingNativeField",
          { field: fieldTitle, row: index + 1 },
          `${fieldTitle} row ${index + 1} is missing a required native field.`
        ) });
      }
    });
    if (fieldKey === "raid_times" && rows.length > 50) {
      issues.push({ fieldKey, reason: "maximum", message: t(
        "scum.settings.validation.raidMaximum", undefined, "SCUM allows at most 50 raid-time entries."
      ) });
    }
  }
  return issues;
}
