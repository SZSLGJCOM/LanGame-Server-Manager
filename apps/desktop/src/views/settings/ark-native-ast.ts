/** Source spans keep unedited native text, unknown properties and Mod classes intact. */
export type ArkNode = ArkScalar | ArkGroup;
export interface ArkScalar { kind: "scalar"; start: number; end: number; raw: string }
export interface ArkEntry { key?: string; start: number; end: number; value: ArkNode }
export interface ArkGroup { kind: "group"; start: number; end: number; entries: ArkEntry[] }
export interface ArkNativeLine { text: string; ending: string }

export function arkLines(raw: string): ArkNativeLine[] {
  return raw.match(/[^\r\n]*(?:\r\n|\r|\n|$)/g)?.filter(Boolean).map((line) => {
    const ending = line.match(/\r\n$|[\r\n]$/)?.[0] ?? "";
    return { text: line.slice(0, line.length - ending.length), ending };
  }) ?? [];
}

export function joinArkLines(lines: ArkNativeLine[]): string {
  return lines.map((line) => line.text + line.ending).join("");
}

export function appendArkLine(raw: string, line: string): string {
  const ending = raw.includes("\r\n") ? "\r\n" : "\n";
  return raw + (raw && !/[\r\n]$/.test(raw) ? ending : "") + line;
}

export function parseArkValue(text: string, offset = 0): ArkNode {
  let at = offset;
  let count = 0;
  const space = () => { while (/\s/.test(text[at] ?? "") && at < text.length) at++; };
  function parse(depth: number): ArkNode {
    if (depth > 48 || ++count > 20000) throw new Error("Native rule is too deeply nested or too large.");
    space();
    const start = at;
    if (text[at] === "(") {
      at++; space();
      const entries: ArkEntry[] = [];
      while (at < text.length && text[at] !== ")") {
        const entryStart = at;
        const keyMatch = text.slice(at).match(/^([A-Za-z_][A-Za-z0-9_.]*(?:\[\d+\])?)\s*=/);
        let key: string | undefined;
        if (keyMatch) { key = keyMatch[1]; at += keyMatch[0].length; }
        const value = parse(depth + 1);
        entries.push({ key, start: entryStart, end: value.end, value });
        space();
        if (text[at] === ")") break;
        if (text[at] !== ",") throw new Error("Expected a comma or closing parenthesis.");
        at++; space();
        if (text[at] === "," || text[at] === ")") throw new Error("Empty native rule entry.");
      }
      if (text[at] !== ")") throw new Error("Missing closing parenthesis.");
      at++;
      return { kind: "group", start, end: at, entries };
    }
    if (text[at] === '"') {
      at++;
      let closed = false;
      while (at < text.length) {
        if (text[at] === "\\") { at += 2; continue; }
        if (text[at++] === '"') { closed = true; break; }
      }
      if (!closed) throw new Error("Unclosed quoted string.");
    } else {
      while (at < text.length && text[at] !== "," && text[at] !== ")") {
        if (text[at] === "(" || text[at] === '"' || text[at] === "=") throw new Error("Unexpected native syntax.");
        at++;
      }
    }
    const end = at;
    const raw = text.slice(start, end).trimEnd();
    if (!raw) throw new Error("Empty native value.");
    return { kind: "scalar", start, end: start + raw.length, raw };
  }
  const result = parse(0); space();
  if (at !== text.length) throw new Error("Unexpected text after native value.");
  return result;
}

export function parseArkRule(text: string, nativeKey: string): ArkGroup {
  const leading = text.length - text.trimStart().length;
  const prefix = `${nativeKey}=`;
  const offset = text.slice(leading).startsWith(prefix) ? leading + prefix.length : leading;
  const node = parseArkValue(text, offset);
  if (node.kind !== "group") throw new Error("Expected a parenthesized native rule.");
  return node;
}

export function replaceArkNode(text: string, node: Pick<ArkNode, "start" | "end">, replacement: string): string {
  return text.slice(0, node.start) + replacement + text.slice(node.end);
}

export function addArkEntry(text: string, group: ArkGroup, entry: string): string {
  return text.slice(0, group.end - 1) + (group.entries.length ? "," : "") + entry + text.slice(group.end - 1);
}

export function removeArkEntry(text: string, group: ArkGroup, index: number): string {
  const current = group.entries[index];
  if (!current) return text;
  const next = group.entries[index + 1];
  const previous = group.entries[index - 1];
  return replaceArkNode(text, { start: next || !previous ? current.start : previous.end,
    end: next ? next.start : current.end }, "");
}

export function arkScalarText(node: ArkScalar): string {
  if (!node.raw.startsWith('"')) return node.raw;
  return node.raw.slice(1, -1).replace(/\\([\\"])/g, "$1");
}

export function quoteArkString(value: string): string {
  return '"' + value.replace(/\\/g, "\\\\").replace(/"/g, '\\"') + '"';
}
