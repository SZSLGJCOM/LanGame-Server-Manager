export type LuaScalar = string | number | boolean | null;
export type LuaValue = LuaScalarNode | LuaTable;
type LuaScalarNode = { kind: "scalar"; value: LuaScalar; start: number; end: number };
type LuaEntry = { key: string | number; value: LuaValue; separator: boolean };
export type LuaTable = {
  kind: "table";
  entries: Map<string | number, LuaEntry>;
  start: number;
  end: number;
  close: number;
  fragment: boolean;
};
type LuaUpdate = LuaScalar | Map<string, LuaScalar>;

const MAX_BYTES = 128 * 1024;
const MAX_DEPTH = 32;
const MAX_ENTRIES = 8192;
const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*/;
const NUMBER = /^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/;
const WHITESPACE = /[ \t\r\n\f\v]/;
const KEYWORDS = new Set([
  "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
  "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while"
]);

// This recognizer accepts data constructors, never Lua expressions or execution.
// Source offsets let edits retain comments, unknown fields and untouched formatting.
class DataParser {
  private position = 0;
  private entries = 0;

  constructor(private readonly source: string) {}

  parse(fragment: boolean): LuaTable {
    this.trivia();
    let table: LuaTable;
    if (fragment) {
      table = this.table(0, true);
    } else {
      if (this.identifier() !== "return") this.fail();
      this.trivia();
      table = this.table(0, false);
      this.trivia();
      if (this.source[this.position] === ";") this.position++;
      this.trivia();
    }
    if (this.position !== this.source.length) this.fail();
    return table;
  }

  private fail(): never { throw new Error("not a bounded Lua data table"); }

  private identifier(): string | null {
    const match = IDENTIFIER.exec(this.source.slice(this.position));
    if (!match) return null;
    this.position += match[0].length;
    return match[0];
  }

  private longString(): string | null {
    const opening = /^\[(=*)\[/.exec(this.source.slice(this.position));
    if (!opening) return null;
    const begin = this.position + opening[0].length;
    const close = `]${opening[1]}]`;
    const end = this.source.indexOf(close, begin);
    if (end < 0) this.fail();
    this.position = end + close.length;
    return this.source.slice(begin, end).replace(/\r\n|\n\r|\r/g, "\n").replace(/^\n/, "");
  }

  private trivia(): void {
    while (this.position < this.source.length) {
      if (WHITESPACE.test(this.source[this.position])) { this.position++; continue; }
      if (!this.source.startsWith("--", this.position)) break;
      this.position += 2;
      if (this.longString() !== null) continue;
      while (this.position < this.source.length && !/[\r\n]/.test(this.source[this.position])) {
        this.position++;
      }
    }
  }

  private quotedString(): string {
    const quote = this.source[this.position++];
    const bytes: number[] = [];
    const appendText = (value: string) => {
      for (const byte of new TextEncoder().encode(value)) bytes.push(byte);
    };
    let literalStart = this.position;
    const escapes: Record<string, string> = {
      a: "\x07", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t", v: "\v", "\\": "\\", "'": "'", '"': '"'
    };
    while (this.position < this.source.length) {
      const character = this.source[this.position++];
      if (character === quote) {
        appendText(this.source.slice(literalStart, this.position - 1));
        return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(Uint8Array.from(bytes));
      }
      if (character === "\n" || character === "\r") this.fail();
      if (character !== "\\") continue;
      appendText(this.source.slice(literalStart, this.position - 1));
      const escaped = this.source[this.position++];
      if (Object.prototype.hasOwnProperty.call(escapes, escaped)) {
        appendText(escapes[escaped]);
      } else if (escaped === "z") {
        while (this.position < this.source.length && WHITESPACE.test(this.source[this.position])) this.position++;
      } else if (escaped === "\n" || escaped === "\r") {
        if ((escaped === "\r" && this.source[this.position] === "\n") ||
            (escaped === "\n" && this.source[this.position] === "\r")) this.position++;
        bytes.push(10);
      } else if (escaped === "x") {
        const hex = this.source.slice(this.position, this.position + 2);
        if (!/^[\da-fA-F]{2}$/.test(hex)) this.fail();
        bytes.push(parseInt(hex, 16));
        this.position += 2;
      } else if (/\d/.test(escaped ?? "")) {
        const digits = /^[0-9]{0,2}/.exec(this.source.slice(this.position))?.[0] ?? "";
        const code = Number(escaped + digits);
        if (code > 255) this.fail();
        bytes.push(code);
        this.position += digits.length;
      } else this.fail();
      literalStart = this.position;
    }
    return this.fail();
  }

  private value(depth: number): LuaValue {
    this.trivia();
    const start = this.position;
    const character = this.source[start];
    if (character === "{") return this.table(depth, false);
    let value: LuaScalar;
    if (character === "'" || character === '"') {
      value = this.quotedString();
    } else if (character === "[" && /^\[=*\[/.test(this.source.slice(start))) {
      const long = this.longString();
      if (long === null) this.fail();
      value = long;
    } else {
      let negative = false;
      if (character === "-") { negative = true; this.position++; this.trivia(); }
      const number = NUMBER.exec(this.source.slice(this.position));
      if (number) {
        this.position += number[0].length;
        value = Number(number[0]) * (negative ? -1 : 1);
        if (!Number.isFinite(value)) this.fail();
      } else {
        if (negative) this.fail();
        const word = this.identifier();
        if (word === "true") value = true;
        else if (word === "false") value = false;
        else if (word === "nil") value = null;
        else return this.fail();
      }
    }
    return { kind: "scalar", value, start, end: this.position };
  }

  private table(depth: number, fragment: boolean): LuaTable {
    if (depth >= MAX_DEPTH) this.fail();
    const start = this.position;
    if (!fragment && this.source[this.position++] !== "{") this.fail();
    const entries = new Map<string | number, LuaEntry>();
    let arrayIndex = 1;
    this.trivia();
    while (this.position < this.source.length && (fragment || this.source[this.position] !== "}")) {
      if (++this.entries > MAX_ENTRIES) this.fail();
      let key: string | number;
      let value: LuaValue;
      const fieldStart = this.position;
      if (this.source[this.position] === "[" && !/^\[=*\[/.test(this.source.slice(this.position))) {
        this.position++;
        const keyNode = this.value(depth + 1);
        if (keyNode.kind !== "scalar" || (typeof keyNode.value !== "string" && typeof keyNode.value !== "number")) this.fail();
        key = keyNode.value;
        this.trivia();
        if (this.source[this.position++] !== "]") this.fail();
        this.trivia();
        if (this.source[this.position++] !== "=") this.fail();
        value = this.value(depth + 1);
      } else {
        const identifier = this.identifier();
        this.trivia();
        if (identifier && !KEYWORDS.has(identifier) && this.source[this.position] === "=") {
          this.position++;
          key = identifier;
          value = this.value(depth + 1);
        } else {
          this.position = fieldStart;
          key = arrayIndex++;
          value = this.value(depth + 1);
        }
      }
      if (entries.has(key)) this.fail();
      this.trivia();
      const separator = /[,;]/.test(this.source[this.position] ?? "");
      entries.set(key, { key, value, separator });
      if (separator) { this.position++; this.trivia(); }
      else if (fragment ? this.position !== this.source.length : this.source[this.position] !== "}") this.fail();
    }
    const close = this.position;
    if (!fragment && this.source[this.position++] !== "}") this.fail();
    return { kind: "table", entries, start, end: this.position, close, fragment };
  }
}

export function parseLuaDataTable(source: string, fragment = false): LuaTable | null {
  if (source.length > MAX_BYTES || new TextEncoder().encode(source).length > MAX_BYTES) return null;
  try { return new DataParser(source).parse(fragment); } catch { return null; }
}

export function luaScalar(table: LuaTable | null, key: string): LuaScalar | undefined {
  const value = table?.entries.get(key)?.value;
  return value?.kind === "scalar" ? value.value : undefined;
}

export function luaChildTable(table: LuaTable, key: string): LuaTable | null {
  const value = table.entries.get(key)?.value;
  return value?.kind === "table" ? value : null;
}

function literal(value: LuaScalar): string {
  if (value === null) return "nil";
  if (typeof value !== "string") return String(value);
  return `"${value.replace(/[\\"\x00-\x1f\x7f]/g, (character) => {
    if (character === "\\" || character === '"') return `\\${character}`;
    return `\\${character.charCodeAt(0).toString().padStart(3, "0")}`;
  })}"`;
}

function keyLiteral(key: string): string {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(key) && !KEYWORDS.has(key) ? key : `[${literal(key)}]`;
}

function updateLiteral(value: LuaUpdate): string {
  return value instanceof Map
    ? `{ ${[...value].map(([key, item]) => `${keyLiteral(key)} = ${literal(item)}`).join(", ")} }`
    : literal(value);
}

export function patchLuaDataTable(
  source: string,
  table: LuaTable,
  updates: ReadonlyMap<string, LuaUpdate>
): string {
  const edits: { start: number; end: number; text: string }[] = [];
  const additions: string[] = [];
  for (const [key, value] of updates) {
    const existing = table.entries.get(key)?.value;
    if (!existing) { additions.push(`${keyLiteral(key)} = ${updateLiteral(value)},`); continue; }
    if (!(value instanceof Map) && existing.kind === "scalar" && existing.value === value) continue;
    let text = updateLiteral(value);
    if (value instanceof Map && existing.kind === "table") {
      const patched = patchLuaDataTable(source, existing, value);
      text = patched.slice(existing.start, patched.length - (source.length - existing.end));
    }
    edits.push({ start: existing.start, end: existing.end, text });
  }
  if (additions.length) {
    const last = [...table.entries.values()].pop();
    const separatorAtClose = last && !last.separator && last.value.end === table.close ? "," : "";
    if (last && !last.separator && !separatorAtClose) edits.push({ start: last.value.end, end: last.value.end, text: "," });
    const newline = source.includes("\r\n") ? "\r\n" : "\n";
    const lineStart = source.lastIndexOf("\n", table.start - 1) + 1;
    const indentation = /^[ \t]*/.exec(source.slice(lineStart, table.start))?.[0] ?? "";
    const indent = table.fragment ? "" : `${indentation}  `;
    const lines = additions.map((line) => `${indent}${line}`).join(newline);
    edits.push({ start: table.close, end: table.close, text: `${separatorAtClose}${newline}${lines}${newline}${table.fragment ? "" : indentation}` });
  }
  let output = source;
  for (const edit of edits.sort((left, right) => right.start - left.start)) {
    output = output.slice(0, edit.start) + edit.text + output.slice(edit.end);
  }
  return output;
}
