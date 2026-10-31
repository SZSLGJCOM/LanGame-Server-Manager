import { useState } from "react";
import { useI18n } from "../../i18n";
import { ArkCommitInput, ArkEditorFrame, arkFieldInputId, arkText, type ArkFieldEditorProps } from "./ArkEditorFrame";
import { ConfigurationValidationNotice } from "./ConfigurationValidationNotice";
import { appendArkLevels, exportArkLevelCsv, importArkLevelCsv, patchArkLevel, readArkEngramPoints, readArkLevels } from "./ark-level-model";
import { appendArkLine, arkLines, joinArkLines } from "./ark-native-ast";

function downloadCsv(contents: string, name: string) {
  const url = URL.createObjectURL(new Blob([contents], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a"); link.href = url; link.download = name; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function ArkLevelsEditor(props: ArkFieldEditorProps) {
  const { t } = useI18n();
  const [count, setCount] = useState("1");
  const [increment, setIncrement] = useState("100");
  const [csv, setCsv] = useState("");
  const [error, setError] = useState<{ message: string; inputId: string; label: string } | null>(null);
  const fieldId = arkFieldInputId(props);
  const countId = `${fieldId}-count`;
  const incrementId = `${fieldId}-increment`;
  const csvId = `${fieldId}-csv`;
  const errorId = `${fieldId}-operation-error`;
  const raw = String(props.settings[props.settingKey] ?? "");
  let curves: ReturnType<typeof readArkLevels>["curves"] = [];
  try { curves = readArkLevels(raw).curves; } catch { /* Frame exposes the untouched native text. */ }
  const firstEditableCurve = curves.findIndex((curve) => curve.levels.length > 0);
  const appendId = (curveIndex: number) => firstEditableCurve === -1 && curveIndex === 0 ? fieldId : `${fieldId}-append-${curveIndex}`;
  function append(curveIndex: number) {
    if (curveIndex === 1 && !curves.length) {
      setError({ message: arkText(t, "playerCurveRequired"), inputId: appendId(0), label: `${arkText(t, "append")} · ${arkText(t, "player")}` }); return;
    }
    try { props.onPatch({ [props.settingKey]: appendArkLevels(raw, curveIndex, Number(count), Number(increment)) }); setError(null); }
    catch {
      const invalidCount = !Number.isInteger(Number(count)) || Number(count) < 1 || Number(count) > 500;
      const invalidIncrement = !Number.isFinite(Number(increment)) || Number(increment) <= 0;
      setError({ message: arkText(t, "appendError"), inputId: invalidCount ? countId : invalidIncrement ? incrementId : appendId(curveIndex),
        label: arkText(t, invalidCount ? "count" : invalidIncrement ? "increment" : "append") });
    }
  }
  return <ArkEditorFrame {...props}>
    <div className="ark-editor__toolbar"><label>{arkText(t, "count")}<input id={countId} className="settings-schema-input" value={count} type="number" min={1} max={500} disabled={props.disabled}
      aria-invalid={error?.inputId === countId || undefined} aria-errormessage={error?.inputId === countId ? errorId : undefined}
      onChange={(event) => { setCount(event.target.value); setError(null); }} /></label>
      <label>{arkText(t, "increment")}<input id={incrementId} className="settings-schema-input" value={increment} type="number" min={0} disabled={props.disabled}
        aria-invalid={error?.inputId === incrementId || undefined} aria-errormessage={error?.inputId === incrementId ? errorId : undefined}
        onChange={(event) => { setIncrement(event.target.value); setError(null); }} /></label></div>
    {[0, 1].map((curveIndex) => {
      const title = arkText(t, curveIndex ? "dino" : "player");
      return <section key={curveIndex} aria-label={title}>
        <div className="ark-editor__heading"><h4>{title}</h4><button type="button" className="secondary-button" disabled={props.disabled}
          id={appendId(curveIndex)} aria-invalid={error?.inputId === appendId(curveIndex) || undefined}
          aria-errormessage={error?.inputId === appendId(curveIndex) ? errorId : undefined}
          aria-label={`${arkText(t, "append")} · ${title}`}
          onClick={() => append(curveIndex)}>{arkText(t, "append")}</button></div>
        {curves[curveIndex]?.levels.length ? <div className="ark-editor__table-scroll"><table className="ark-editor__table">
          <thead><tr><th>{arkText(t, "level")}</th><th>{arkText(t, "xp")}</th><th /></tr></thead>
          <tbody>{curves[curveIndex].levels.map((level, row) => <tr key={level.index}><th scope="row">{level.index}</th><td>
            <ArkCommitInput id={curveIndex === firstEditableCurve && row === 0 ? arkFieldInputId(props) : undefined}
              value={String(level.xp)} numeric disabled={props.disabled} label={`${title} ${level.index} ${arkText(t, "xp")}`}
              onCommit={(value) => props.onPatch({ [props.settingKey]: patchArkLevel(raw, curveIndex, level.index, value) })} />
          </td><td><button type="button" className="ghost-button ark-editor__icon-button" disabled={props.disabled}
            aria-label={arkText(t, "remove", { name: `${title} ${level.index}` })}
            onClick={() => props.onPatch({ [props.settingKey]: patchArkLevel(raw, curveIndex, level.index, "") })}>×</button></td></tr>)}</tbody>
        </table></div> : <p className="ark-editor__hint">{arkText(t, "empty")}</p>}
      </section>;
    })}
    {curves.length > 2 ? <p className="ark-editor__hint">{arkText(t, "extraCurves")}</p> : null}
    <details><summary>{arkText(t, "csv")}</summary>
      <textarea id={csvId} className="settings-schema-input ark-editor__csv" aria-label={arkText(t, "csvInput")} value={csv} disabled={props.disabled}
        aria-invalid={error?.inputId === csvId || undefined} aria-errormessage={error?.inputId === csvId ? errorId : undefined}
        placeholder="curve,level,xp&#10;player,0,100&#10;dino,0,100" onChange={(event) => { setCsv(event.target.value); setError(null); }} />
      <div className="ark-editor__toolbar"><button type="button" className="secondary-button" disabled={props.disabled || !csv.trim()}
        onClick={() => { try { props.onPatch({ [props.settingKey]: importArkLevelCsv(raw, csv, "append") }); setCsv(""); setError(null); }
          catch { setError({ message: arkText(t, "csvError"), inputId: csvId, label: arkText(t, "csvInput") }); } }}>{arkText(t, "csvImport")}</button>
        <button type="button" className="secondary-button" disabled={props.disabled} onClick={() => downloadCsv(exportArkLevelCsv(raw), "ark-levels.csv")}>{arkText(t, "csvExport")}</button></div>
    </details>
    {error ? <><span id={errorId} className="configuration-field-error-a11y">{error.message}</span>
      <ConfigurationValidationNotice {...error} /></> : null}
  </ArkEditorFrame>;
}

export function ArkEngramPointsEditor(props: ArkFieldEditorProps) {
  const { t } = useI18n();
  const [csv, setCsv] = useState("");
  const [error, setError] = useState(false);
  const csvId = `${arkFieldInputId(props)}-csv`;
  const errorMessage = error ? arkText(t, "pointsCsvError") : null;
  const raw = String(props.settings[props.settingKey] ?? "");
  let points: number[] = [];
  try { points = readArkEngramPoints(raw); } catch { /* Frame exposes native recovery. */ }
  function edit(index: number, value: string) {
    const lines = arkLines(raw);
    const lineIndex = lines.map((line, i) => line.text.trim() ? i : -1).filter((i) => i >= 0)[index];
    if (value === "") lines.splice(lineIndex, 1);
    else lines[lineIndex].text = lines[lineIndex].text.replace(/(\d+)\s*$/, value);
    props.onPatch({ [props.settingKey]: joinArkLines(lines) });
  }
  function importCsv() {
    try {
      const rows = csv.replace(/^\uFEFF/, "").trim().split(/\r?\n/);
      if (rows.shift()?.trim() !== "level,points" || rows.length > 1000) throw new Error("csv");
      let next = raw; let index = points.length;
      for (const row of rows) {
        const cells = row.split(",").map((cell) => cell.trim());
        if (cells.length !== 2 || !/^\d+$/.test(cells[0]) || Number(cells[0]) !== index++ || !/^\d+$/.test(cells[1]) || !Number.isSafeInteger(Number(cells[1]))) throw new Error("csv");
        next = appendArkLine(next, cells[1]);
      }
      props.onPatch({ [props.settingKey]: next }); setCsv(""); setError(false);
    } catch { setError(true); }
  }
  return <ArkEditorFrame {...props}>
    <div className="ark-editor__table-scroll"><table className="ark-editor__table"><thead><tr><th>{arkText(t, "level")}</th><th>{arkText(t, "points")}</th><th /></tr></thead>
      <tbody>{points.map((value, index) => <tr key={index}><th scope="row">{index}</th><td>
        <ArkCommitInput id={index === 0 ? arkFieldInputId(props) : undefined} value={String(value)} numeric disabled={props.disabled}
          label={`${arkText(t, "points")} ${index}`} onCommit={(value) => edit(index, value)} /></td>
        <td><button type="button" className="ghost-button ark-editor__icon-button" disabled={props.disabled} aria-label={arkText(t, "remove", { name: String(index) })} onClick={() => edit(index, "")}>×</button></td></tr>)}</tbody>
    </table></div>
    <button id={points.length === 0 ? arkFieldInputId(props) : undefined} type="button" className="secondary-button" disabled={props.disabled}
      onClick={() => props.onPatch({ [props.settingKey]: appendArkLine(raw, "0") })}>{arkText(t, "addPoints")}</button>
    <details><summary>{arkText(t, "csv")}</summary><textarea id={csvId} className="settings-schema-input ark-editor__csv" aria-label={arkText(t, "pointsCsv")} value={csv} disabled={props.disabled}
      aria-invalid={error || undefined} aria-errormessage={error ? `${csvId}-error` : undefined}
      placeholder="level,points" onChange={(event) => { setCsv(event.target.value); setError(false); }} />
      <div className="ark-editor__toolbar"><button type="button" className="secondary-button" disabled={props.disabled || !csv.trim()} onClick={importCsv}>{arkText(t, "csvImport")}</button>
        <button type="button" className="secondary-button" disabled={props.disabled} onClick={() => downloadCsv(["level,points", ...points.map((value, i) => `${i},${value}`)].join("\n"), "ark-engrams.csv")}>{arkText(t, "csvExport")}</button></div>
    </details>{errorMessage ? <span id={`${csvId}-error`} className="configuration-field-error-a11y">{errorMessage}</span> : null}
    <ConfigurationValidationNotice message={errorMessage} inputId={csvId} label={arkText(t, "pointsCsv")} />
  </ArkEditorFrame>;
}
