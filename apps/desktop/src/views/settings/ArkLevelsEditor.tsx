import { useState } from "react";
import { useI18n } from "../../i18n";
import { ArkCommitInput, ArkEditorFrame, arkFieldInputId, arkText, type ArkFieldEditorProps } from "./ArkEditorFrame";
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
  const [error, setError] = useState("");
  const raw = String(props.settings[props.settingKey] ?? "");
  let curves: ReturnType<typeof readArkLevels>["curves"] = [];
  try { curves = readArkLevels(raw).curves; } catch { /* Frame exposes the untouched native text. */ }
  const firstEditableCurve = curves.findIndex((curve) => curve.levels.length > 0);
  function append(curveIndex: number) {
    if (curveIndex === 1 && !curves.length) { setError(arkText(t, "playerCurveRequired")); return; }
    try { props.onPatch({ [props.settingKey]: appendArkLevels(raw, curveIndex, Number(count), Number(increment)) }); setError(""); }
    catch { setError(arkText(t, "appendError")); }
  }
  return <ArkEditorFrame {...props}>
    <div className="ark-editor__toolbar"><label>{arkText(t, "count")}<input className="settings-schema-input" value={count} type="number" min={1} max={500} disabled={props.disabled} onChange={(event) => setCount(event.target.value)} /></label>
      <label>{arkText(t, "increment")}<input className="settings-schema-input" value={increment} type="number" min={0} disabled={props.disabled} onChange={(event) => setIncrement(event.target.value)} /></label></div>
    {[0, 1].map((curveIndex) => {
      const title = arkText(t, curveIndex ? "dino" : "player");
      return <section key={curveIndex} aria-label={title}>
        <div className="ark-editor__heading"><h4>{title}</h4><button type="button" className="secondary-button" disabled={props.disabled}
          id={firstEditableCurve === -1 && curveIndex === 0 ? arkFieldInputId(props) : undefined}
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
      <textarea className="settings-schema-input ark-editor__csv" aria-label={arkText(t, "csvInput")} value={csv} disabled={props.disabled}
        placeholder="curve,level,xp&#10;player,0,100&#10;dino,0,100" onChange={(event) => setCsv(event.target.value)} />
      <div className="ark-editor__toolbar"><button type="button" className="secondary-button" disabled={props.disabled || !csv.trim()}
        onClick={() => { try { props.onPatch({ [props.settingKey]: importArkLevelCsv(raw, csv, "append") }); setCsv(""); setError(""); } catch { setError(arkText(t, "csvError")); } }}>{arkText(t, "csvImport")}</button>
        <button type="button" className="secondary-button" disabled={props.disabled} onClick={() => downloadCsv(exportArkLevelCsv(raw), "ark-levels.csv")}>{arkText(t, "csvExport")}</button></div>
    </details>
    {error ? <p role="alert" className="ark-editor__error">{error}</p> : null}
  </ArkEditorFrame>;
}

export function ArkEngramPointsEditor(props: ArkFieldEditorProps) {
  const { t } = useI18n();
  const [csv, setCsv] = useState("");
  const [error, setError] = useState(false);
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
    <details><summary>{arkText(t, "csv")}</summary><textarea className="settings-schema-input ark-editor__csv" aria-label={arkText(t, "pointsCsv")} value={csv} disabled={props.disabled}
      placeholder="level,points" onChange={(event) => setCsv(event.target.value)} />
      <div className="ark-editor__toolbar"><button type="button" className="secondary-button" disabled={props.disabled || !csv.trim()} onClick={importCsv}>{arkText(t, "csvImport")}</button>
        <button type="button" className="secondary-button" disabled={props.disabled} onClick={() => downloadCsv(["level,points", ...points.map((value, i) => `${i},${value}`)].join("\n"), "ark-engrams.csv")}>{arkText(t, "csvExport")}</button></div>
    </details>{error ? <p className="ark-editor__error" role="alert">{arkText(t, "pointsCsvError")}</p> : null}
  </ArkEditorFrame>;
}
