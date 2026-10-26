export function terrariaReadString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function terrariaReadBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  if (typeof value === "string") {
    return !["", "0", "false", "off", "no"].includes(value.trim().toLowerCase());
  }
  return Boolean(value);
}
