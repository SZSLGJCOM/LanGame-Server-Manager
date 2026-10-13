export const GM_TOOL_MODULES = {
  ark: ["arksurvivalascended", "arksurvivalevolved"],
  dontstarve: ["dontstarve"]
};

export function moduleHasGmTools(moduleId?: string | null): boolean {
  const normalized = String(moduleId ?? "").trim().toLowerCase();
  return Object.values(GM_TOOL_MODULES).some((modules) => modules.includes(normalized));
}
