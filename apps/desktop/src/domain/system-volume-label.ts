/** Keep displayable Windows mount paths; volume GUIDs remain internal identifiers. */
export function displayVolumePath(value: string | null | undefined): string | null {
  if (typeof value !== "string") return null;
  let path = (value ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").trim()
    .replace(/^\\\\\?\\/, "").replace(/\//g, "\\");
  if (!path || /Volume\{[^}]+\}|^[{][\da-f-]+[}]$/i.test(path)) return null;
  if (/^UNC\\/i.test(path)) path = `\\\\${path.slice(4)}`;
  if (/^[a-z]:/i.test(path) || /^\\\\[^\\]+\\[^\\]+/.test(path)) return path.replace(/\\+$/, "");
  return null;
}
