import { useI18n } from "../../i18n";
import { ArkCommitInput, ArkEditorFrame, arkFieldInputId, arkText, type ArkFieldEditorProps } from "./ArkEditorFrame";
import { ARK_STAT_NAMES, patchArkStat, readArkStats } from "./ark-stat-model";

export function ArkStatsEditor(props: ArkFieldEditorProps) {
  const { t } = useI18n();
  const raw = String(props.settings[props.settingKey] ?? "");
  let entries: ReturnType<typeof readArkStats>["entries"] = [];
  try { entries = readArkStats(raw, props.settingKey).entries; } catch { /* Original text remains editable in the frame. */ }
  const variants = props.settingKey === "per_level_stats_multiplier_dino_tamed_type_integer" ? ["", "_Add", "_Affinity"] : [""];
  const indexes = [...new Set([...ARK_STAT_NAMES.map((_, i) => i), ...entries.map((entry) => entry.index)])].sort((a, b) => a - b);
  return <ArkEditorFrame {...props}><div className="ark-editor__table-scroll"><table className="ark-editor__table">
    <thead><tr><th>{arkText(t, "stat")}</th>{variants.map((variant, i) => <th key={variant}>{arkText(t, ["base", "addition", "affinity"][i])}</th>)}</tr></thead>
    <tbody>{indexes.map((index) => {
      const label = index < ARK_STAT_NAMES.length ? arkText(t, `stat.${ARK_STAT_NAMES[index]}`) : arkText(t, "extraStat", { index });
      return <tr key={index}><th scope="row">{label} <small>[{index}]</small></th>{variants.map((variant, i) => <td key={variant}>
        <ArkCommitInput id={index === 0 && i === 0 ? arkFieldInputId(props) : undefined} value={entries.find((entry) => entry.index === index && entry.variant === variant)?.value ?? ""}
          label={`${label} ${arkText(t, ["base", "addition", "affinity"][i])}`} numeric disabled={props.disabled}
          placeholder={arkText(t, "default")} onCommit={(value) => props.onPatch({ [props.settingKey]: patchArkStat(raw, props.settingKey, index, variant, value) })} />
      </td>)}</tr>;
    })}</tbody>
  </table></div></ArkEditorFrame>;
}
