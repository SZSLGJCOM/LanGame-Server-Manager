import { useId, useState } from "react";
import { useI18n } from "../../i18n";
import { ARK_EDITORS_EN } from "../../i18n/games/ark-editors.en";
import { ArkCommitInput, arkText } from "./ArkEditorFrame";
import { addArkEntry, arkScalarText, quoteArkString, removeArkEntry, replaceArkNode, type ArkGroup, type ArkScalar } from "./ark-native-ast";
import { arkClassCatalogKind, searchArkClasses, type ArkCatalogKind } from "./ark-class-catalog";

const NATIVE_GROUPS = new Set(["ItemSets", "ItemEntries", "BaseCraftingResourceRequirements", "Quantity", "NPCSpawnEntries", "NPCSpawnLimits", "ItemClassStrings", "ItemsWeights", "NPCsToSpawnStrings"]);

function ClassInput(props: { node: ArkScalar; text: string; label: string; kind: ArkCatalogKind; disabled: boolean; id?: string; onChange: (text: string) => void }) {
  const listId = useId();
  const current = arkScalarText(props.node);
  const [draft, setDraft] = useState<string | null>(null);
  const options = searchArkClasses(props.kind, draft ?? current);
  return <><input id={props.id} className="settings-schema-input" aria-label={props.label} disabled={props.disabled}
    list={listId} value={draft ?? current} onChange={(event) => setDraft(event.target.value)}
    onBlur={() => { if (draft !== null) { props.onChange(replaceArkNode(props.text, props.node, quoteArkString(draft))); setDraft(null); } }}
    onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} />
    <datalist id={listId}>{options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</datalist></>;
}

export interface ArkRuleNodeEditorProps { group: ArkGroup; text: string; path: string; property?: string; inputId?: string;
  disabled: boolean; onChange: (text: string) => void; depth?: number; nativeContext?: boolean }

export function ArkRuleNodeEditor(props: ArkRuleNodeEditorProps) {
  const { t } = useI18n();
  const [newKey, setNewKey] = useState("");
  const [invalid, setInvalid] = useState(false);
  const isArray = props.group.entries.length > 0 && props.group.entries.every((entry) => !entry.key);
  const depth = props.depth ?? 0;
  // Large or deeply nested documents remain accessible through native text without building a huge DOM.
  if (depth > 12 || props.group.entries.length > 200) return <div><p className="ark-editor__hint">{arkText(t, "largeNative")}</p>
    <textarea id={props.inputId} className="settings-schema-input" aria-label={props.path} readOnly value={props.text.slice(props.group.start, props.group.end)} /></div>;
  function addProperty() {
    if (!/^[A-Za-z_][A-Za-z0-9_]*(?:\[\d+\])?$/.test(newKey) || props.group.entries.some((entry) => entry.key === newKey)) { setInvalid(true); return; }
    props.onChange(addArkEntry(props.text, props.group, `${newKey}=""`)); setNewKey(""); setInvalid(false);
  }
  return <div className="ark-editor__properties">
    {props.group.entries.map((entry, index) => {
      const property = entry.key ?? props.property ?? "";
      const label = entry.key ?? `${arkText(t, "value")} ${index + 1}`;
      const nativeContext = props.nativeContext !== false;
      const translated = nativeContext && entry.key && ARK_EDITORS_EN[`arkEditor.property.${entry.key}`];
      const displayLabel = translated ? <span className="ark-editor__property-name"><span>{arkText(t, `property.${entry.key}`)}</span><code>{entry.key}</code></span> : label;
      const path = `${props.path} / ${label}`;
      const inputId = index === 0 ? props.inputId : undefined;
      const node = entry.value;
      const remove = <button className="ghost-button ark-editor__icon-button" type="button" disabled={props.disabled} aria-label={arkText(t, "remove", { name: path })}
        onClick={(event) => { event.preventDefault(); event.stopPropagation(); props.onChange(removeArkEntry(props.text, props.group, index)); }}>×</button>;
      return <div className="ark-editor__property" key={`${entry.key ?? index}:${index}`}>
        {node.kind === "group" ? <details open={depth < 1 || Boolean(inputId)} className="ark-editor__nested"><summary className="ark-editor__group-heading"><span>{displayLabel}</span>{remove}</summary>
          <ArkRuleNodeEditor group={node} text={props.text} path={path} property={property} inputId={inputId}
            disabled={props.disabled} onChange={props.onChange} depth={depth + 1} nativeContext={nativeContext && (!entry.key || NATIVE_GROUPS.has(entry.key))} />
        </details> : <><label>{displayLabel}</label>{arkClassCatalogKind(property)
          ? <ClassInput node={node} text={props.text} label={path} kind={arkClassCatalogKind(property)!} id={inputId} disabled={props.disabled} onChange={props.onChange} />
          : /^(true|false)$/i.test(node.raw)
            ? <select id={inputId} className="settings-schema-input" aria-label={path} disabled={props.disabled} value={node.raw.toLowerCase()}
              onChange={(event) => props.onChange(replaceArkNode(props.text, node, event.target.value === "true" ? "True" : "False"))}>
              <option value="true">True</option><option value="false">False</option></select>
            : <ArkCommitInput id={inputId} value={arkScalarText(node)} label={path} disabled={props.disabled}
              numeric={!node.raw.startsWith('"') && Number.isFinite(Number(node.raw))}
              onCommit={(value) => props.onChange(replaceArkNode(props.text, node, node.raw.startsWith('"') ? quoteArkString(value) : value))} />}{remove}</>}
      </div>;
    })}
    {isArray ? <button className="secondary-button" type="button" disabled={props.disabled} onClick={() => {
      const last = props.group.entries[props.group.entries.length - 1].value;
      props.onChange(addArkEntry(props.text, props.group, props.text.slice(last.start, last.end)));
    }}>{arkText(t, "addEntry")}</button> : <div className="ark-editor__toolbar">
      <input id={props.group.entries.length === 0 ? props.inputId : undefined} className="settings-schema-input" aria-label={`${props.path} ${arkText(t, "propertyName")}`}
        placeholder={arkText(t, "propertyName")} disabled={props.disabled} value={newKey} onChange={(event) => setNewKey(event.target.value)} />
      <button className="secondary-button" type="button" disabled={props.disabled || !newKey} onClick={addProperty}>{arkText(t, "addProperty")}</button>
    </div>}
    {invalid ? <p className="ark-editor__error" role="alert">{arkText(t, "propertyError")}</p> : null}
  </div>;
}
