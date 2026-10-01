import { luaChildTable, luaScalar, parseLuaDataTable } from "../settings/modules/dontstarve-lua-data";

export interface DstRawModDeclaration {
  id: string;
  enabled: boolean;
  options: Record<string, string | number | boolean>;
}

/** Read bounded literal declarations without executing or rewriting imported Lua. */
export function readDstRawModDeclarations(raw: unknown): DstRawModDeclaration[] {
  if (typeof raw !== "string") return [];
  const table = parseLuaDataTable(raw);
  if (!table) return [];
  const declarations: DstRawModDeclaration[] = [];
  for (const [key] of table.entries) {
    if (typeof key !== "string") continue;
    const id = key.replace(/^workshop-/u, "");
    if (!/^[1-9]\d{0,19}$/u.test(id) || BigInt(id) > 18446744073709551615n) continue;
    const declaration = luaChildTable(table, key);
    if (!declaration) continue;
    const options: DstRawModDeclaration["options"] = {};
    const optionTable = luaChildTable(declaration, "configuration_options");
    for (const [name, entry] of optionTable?.entries ?? []) {
      if (typeof name !== "string" || entry.value.kind !== "scalar") continue;
      const value = entry.value.value;
      if (typeof value === "string" || typeof value === "boolean" ||
        (typeof value === "number" && Number.isFinite(value))) options[name] = value;
    }
    declarations.push({ id, enabled: luaScalar(declaration, "enabled") === true, options });
  }
  return declarations;
}
