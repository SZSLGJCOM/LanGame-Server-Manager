import type { ReactNode } from "react";
import type { ArkMapPlayer } from "../../ark-world-map";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import { ArkCatalogPicker } from "./ArkCatalogPicker";
import type { GmToolDefinition, GmToolField } from "./gm-tools";

interface Props {
  tool: GmToolDefinition; values: Record<string, string>; query: string; onQueryChange: (query: string) => void;
  update: (field: GmToolField, value: string) => void; selectedPlayer: ArkMapPlayer | null;
  targetMode: "player" | "manual"; onTargetModeChange: (mode: "player" | "manual") => void;
  busy: boolean; sending: boolean; issue: string | null; onSubmit: () => void; feedback: ReactNode; result: ReactNode;
}

/** The picker takes the remaining rail height; the compact controls remain at the bottom. */
export function ArkItemSpawnPanel(props: Props) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const { tool, values, update } = props;
  const mode = values.itemMode || "blueprint";
  const itemKey = mode === "number" ? "itemId" : "blueprintPath";
  const field = (key: string) => tool.fields.find(value => value.key === key);
  const change = (key: string, value: string) => { const definition = field(key); if (definition) update(definition, value); };
  const targetHelp = props.targetMode === "manual"
    ? text(`发放给手动指定的玩家 ID ${values.playerId || ""}。`, `Give to the manually entered player ID ${values.playerId || ""}.`)
    : text(`发放给所选玩家 ${props.selectedPlayer?.name || props.selectedPlayer?.playerId || ""}。`,
      `Give to the selected player ${props.selectedPlayer?.name || props.selectedPlayer?.playerId || ""}.`);
  function control(key: string, compactLabel?: string) {
    const definition = field(key);
    if (!definition) return null;
    const id = `gm-${tool.id}-${key}-input`;
    const label = t(`servers.gmTools.fields.${tool.id}.${key}`, undefined, definition.label);
    const value = values[key] ?? definition.defaultValue ?? "";
    return <label className={compactLabel ? "ark-spawn-setting" : "gmt-field"} htmlFor={id} title={label}>
      <span className={compactLabel ? undefined : "gmt-field-label detail-label"}>{compactLabel || label}</span>
      {definition.type === "select" ? <select id={id} className={compactLabel ? undefined : "gmt-select"} value={value} onChange={event => update(definition, event.target.value)}>
        {definition.options?.map(option => <option key={option.value} value={option.value}>{option.labelKey ? t(option.labelKey, undefined, option.label) : option.label}</option>)}
      </select> : definition.type === "textarea" ? <textarea id={id} className="gmt-textarea" value={value} placeholder={definition.placeholder}
        onChange={event => update(definition, event.target.value)} />
        : <input id={id} className={compactLabel ? undefined : "gmt-text-input"} type={definition.type === "number" ? "number" : "text"}
          value={value} min={definition.min} max={definition.max} step={definition.step} placeholder={definition.placeholder}
          onChange={event => update(definition, event.target.value)} />}
    </label>;
  }
  return <form className="gmt-form ark-item-spawn" aria-label={text("发放物品", "Give items")} onSubmit={event => { event.preventDefault(); props.onSubmit(); }}>
    {mode === "batch" ? <div className="ark-item-spawn__batch">{control("lines")}</div>
      : <ArkCatalogPicker key={mode} kind={mode === "number" ? "number" : "blueprint"} query={props.query} onQueryChange={props.onQueryChange}
        value={values[itemKey] ?? ""} onPick={value => change(itemKey, value)} />}
    <div className="ark-spawn-dock">
      <details className="ark-spawn-options"><summary>{props.targetMode === "manual"
        ? text("手动目标已启用 · 自定义物品", "Manual target active · Custom item")
        : text("手动目标与自定义物品", "Manual target and custom item")}</summary>
        <div className="gmt-fields-grid">
          <label className="gmt-field"><span className="gmt-field-label detail-label">{text("发放目标", "Recipient")}</span>
            <select className="gmt-select" aria-label={text("发放目标来源", "Recipient source")} value={props.targetMode}
              onChange={event => props.onTargetModeChange(event.target.value === "manual" ? "manual" : "player")}>
              <option value="player">{text("当前所选玩家", "Currently selected player")}</option>
              <option value="manual">{text("手动输入玩家 ID", "Enter player ID manually")}</option>
            </select>
          </label>
          {props.targetMode === "manual" ? control("playerId") : null}{control("itemMode")}{mode !== "batch" ? control(itemKey) : null}
        </div>
      </details>
      <div className="ark-spawn-tool-grid">
        {mode === "batch" ? <div className="ark-item-spawn__batch-label">{text("按上方清单逐行发放", "Give each line above")}</div> : <>
          <button type="button" className="secondary-button ark-spawn-tool" aria-pressed={values.blueprint === "1"}
            title={text("开启后发放蓝图，关闭后发放物品", "Give a blueprint when enabled, or the item when disabled")}
            onClick={() => change("blueprint", values.blueprint === "1" ? "0" : "1")}><ShellIcon name="file-text" /><span>{text("蓝图", "Blueprint")}</span></button>
          {control("quantity", text("数量", "Amount"))}{control("quality", text("品质", "Quality"))}
        </>}
        <ConfigurationActionHelp reason={props.issue || targetHelp}><button type="submit" className="primary-button ark-spawn-tool" disabled={props.busy || Boolean(props.issue)}>
          <ShellIcon name={props.sending ? "loader" : "send"} /><span>{props.sending ? text("发送中", "Sending") : text("发放", "Give")}</span>
        </button></ConfigurationActionHelp>
      </div>
      {props.feedback}{props.result}
    </div>
  </form>;
}
