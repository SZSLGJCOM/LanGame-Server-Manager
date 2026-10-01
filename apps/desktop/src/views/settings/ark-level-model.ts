import { addArkEntry, arkLines, joinArkLines, parseArkRule, removeArkEntry, replaceArkNode, type ArkGroup } from "./ark-native-ast";

export interface ArkLevel { index: number; xp: number; entryIndex: number }
export interface ArkCurve { lineIndex: number; node: ArkGroup; levels: ArkLevel[] }
export function readArkLevels(raw: string): { curves: ArkCurve[] } {
  const curves: ArkCurve[] = [];
  for (const [lineIndex, line] of arkLines(raw).entries()) {
    if (!line.text.trim()) continue;
    const node = parseArkRule(line.text, "LevelExperienceRampOverrides");
    const levels: ArkLevel[] = [];
    const seen = new Set<number>();
    node.entries.forEach((entry, entryIndex) => {
      const match = entry.key?.match(/^ExperiencePointsForLevel\[(\d+)\]$/);
      if (!match) {
        if (entry.key?.startsWith("ExperiencePointsForLevel")) throw new Error("Invalid level index.");
        return;
      }
      const index = Number(match[1]);
      const xp = entry.value.kind === "scalar" ? Number(entry.value.raw) : NaN;
      if (!Number.isSafeInteger(index) || seen.has(index) || !Number.isFinite(xp) || xp < 0) throw new Error("Invalid or duplicate level / XP.");
      seen.add(index); levels.push({ index, xp, entryIndex });
    });
    levels.sort((a, b) => a.index - b.index);
    for (let index = 1; index < levels.length; index++) {
      if (levels[index].xp <= levels[index - 1].xp) throw new Error("Experience must increase with each level.");
    }
    curves.push({ lineIndex, node, levels });
  }
  return { curves };
}

export function patchArkLevel(raw: string, curveIndex: number, level: number, xp: string): string {
  const curve = readArkLevels(raw).curves[curveIndex];
  if (!curve) throw new Error("Curve does not exist.");
  const lines = arkLines(raw);
  const existing = curve.levels.find((entry) => entry.index === level);
  const text = lines[curve.lineIndex].text;
  lines[curve.lineIndex].text = existing
    ? xp === "" ? removeArkEntry(text, curve.node, existing.entryIndex)
      : replaceArkNode(text, curve.node.entries[existing.entryIndex].value, xp)
    : addArkEntry(text, curve.node, `ExperiencePointsForLevel[${level}]=${xp}`);
  return joinArkLines(lines);
}

export function appendArkLevels(raw: string, curveIndex: number, count: number, increment: number): string {
  if (!Number.isInteger(count) || count < 1 || count > 500 || !Number.isFinite(increment) || increment <= 0) throw new Error("Append 1–500 levels with a positive XP increment.");
  let output = raw;
  let curves = readArkLevels(output).curves;
  if (curveIndex < 0 || curveIndex > 1) throw new Error("Choose player or dino curve.");
  if (curveIndex === 1 && !curves.length) throw new Error("Create the player curve before adding a dino curve.");
  while (curves.length <= curveIndex) {
    output += (output && !/[\r\n]$/.test(output) ? "\n" : "") + "()";
    curves = readArkLevels(output).curves;
  }
  const currentLevels = curves[curveIndex].levels;
  const last = currentLevels[currentLevels.length - 1];
  let level = last ? last.index + 1 : 0;
  let xp = last?.xp ?? 0;
  for (let i = 0; i < count; i++, level++) {
    xp += increment;
    if (!Number.isFinite(xp)) throw new Error("XP exceeds numeric limits.");
    output = patchArkLevel(output, curveIndex, level, String(xp));
  }
  return output;
}

export function exportArkLevelCsv(raw: string): string {
  return ["curve,level,xp", ...readArkLevels(raw).curves.slice(0, 2).flatMap((curve, i) =>
    curve.levels.map((level) => `${i ? "dino" : "player"},${level.index},${level.xp}`))].join("\n");
}

/** Import is append-only, so a paste never silently overwrites an existing curve. */
export function importArkLevelCsv(raw: string, csv: string, _mode: "append"): string {
  const rows = csv.replace(/^\uFEFF/, "").trim().split(/\r?\n/);
  if (rows.shift()?.trim().toLowerCase() !== "curve,level,xp") throw new Error("Expected CSV header curve,level,xp.");
  if (rows.length > 1000) throw new Error("Import at most 1000 levels.");
  let output = raw;
  for (const row of rows) {
    if (!row.trim()) continue;
    const cells = row.split(",").map((cell) => cell.trim());
    const curveIndex = cells[0] === "player" ? 0 : cells[0] === "dino" ? 1 : -1;
    const index = Number(cells[1]); const xp = Number(cells[2]);
    if (cells.length !== 3 || curveIndex < 0 || !/^\d+$/.test(cells[1]) || !Number.isSafeInteger(index) || !cells[2] || !Number.isFinite(xp) || xp < 0) throw new Error("Invalid CSV row.");
    let curves = readArkLevels(output).curves;
    if (curveIndex === 1 && !curves.length) throw new Error("Create the player curve first.");
    while (curves.length <= curveIndex) {
      output += (output && !/[\r\n]$/.test(output) ? "\n" : "") + "()";
      curves = readArkLevels(output).curves;
    }
    if (curves[curveIndex].levels.some((level) => level.index >= index)) throw new Error("Level already exists or precedes existing levels.");
    output = patchArkLevel(output, curveIndex, index, String(xp));
    readArkLevels(output);
  }
  return output;
}

export function readArkEngramPoints(raw: string): number[] {
  return arkLines(raw).filter((line) => line.text.trim()).map((line) => {
    const text = line.text.trim().replace(/^OverridePlayerLevelEngramPoints\s*=\s*/, "");
    if (!/^\d+$/.test(text) || !Number.isSafeInteger(Number(text))) throw new Error("Engram points must be non-negative integers.");
    return Number(text);
  });
}
