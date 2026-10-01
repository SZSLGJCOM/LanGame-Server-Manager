import { useI18n } from "../../i18n";
import { ArkEditorFrame, arkFieldInputId, arkText, type ArkFieldEditorProps } from "./ArkEditorFrame";
import { ArkRuleNodeEditor } from "./ArkRuleNodeEditor";
import { appendArkLine, arkLines, joinArkLines, parseArkRule, type ArkGroup } from "./ark-native-ast";
import { ARK_RULE_FIELDS } from "./ark-rule-fields";

export function ArkRulesEditor(props: ArkFieldEditorProps) {
  const { t } = useI18n();
  const raw = String(props.settings[props.settingKey] ?? "");
  const field = ARK_RULE_FIELDS[props.settingKey];
  const lines = arkLines(raw);
  const rules: { lineIndex: number; node: ArkGroup }[] = [];
  try { for (const [lineIndex, line] of lines.entries()) if (line.text.trim()) rules.push({ lineIndex, node: parseArkRule(line.text, field.nativeKey) }); }
  catch { /* The frame renders the unmodified original source. */ }
  const patch = () => props.onPatch({ [props.settingKey]: joinArkLines(lines) });
  return <ArkEditorFrame {...props}>
    {rules.slice(0, 100).map((rule, index) => {
      const name = arkText(t, "rule", { number: index + 1 });
      return <section className="ark-editor__rule" key={index} aria-label={name}>
        <div className="ark-editor__heading"><h4>{name}</h4><div className="ark-editor__toolbar">
          <button type="button" className="ghost-button ark-editor__icon-button" disabled={props.disabled || index === 0} aria-label={arkText(t, "up", { name })}
            onClick={() => { const other = rules[index - 1].lineIndex; [lines[other].text, lines[rule.lineIndex].text] = [lines[rule.lineIndex].text, lines[other].text]; patch(); }}>↑</button>
          <button type="button" className="ghost-button ark-editor__icon-button" disabled={props.disabled || index === rules.length - 1} aria-label={arkText(t, "down", { name })}
            onClick={() => { const other = rules[index + 1].lineIndex; [lines[other].text, lines[rule.lineIndex].text] = [lines[rule.lineIndex].text, lines[other].text]; patch(); }}>↓</button>
          <button type="button" className="ghost-button ark-editor__icon-button" disabled={props.disabled} aria-label={arkText(t, "remove", { name })}
            onClick={() => { lines.splice(rule.lineIndex, 1); patch(); }}>×</button>
        </div></div>
        <ArkRuleNodeEditor group={rule.node} text={lines[rule.lineIndex].text} path={name} inputId={index === 0 ? arkFieldInputId(props) : undefined}
          disabled={props.disabled} onChange={(text) => { lines[rule.lineIndex].text = text; patch(); }} />
      </section>;
    })}
    {rules.length > 100 ? <p className="ark-editor__hint">{arkText(t, "largeNative")}</p> : null}
    <button id={rules.length === 0 ? arkFieldInputId(props) : undefined} type="button" className="secondary-button" disabled={props.disabled}
      onClick={() => props.onPatch({ [props.settingKey]: appendArkLine(raw, field.sample) })}>{arkText(t, "add")}</button>
  </ArkEditorFrame>;
}
