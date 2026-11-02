import type { AppPathSettingsInput } from "../../types";

export function defaultArchiveRoot(workspace: string): string {
  const root = workspace.trim();
  return `${root.replace(/[\\/]+$/, "")}${root.includes("\\") ? "\\" : "/"}.trash`;
}

export function createAppPathSettingsDraft(settings: AppPathSettingsInput): AppPathSettingsInput {
  return {
    games_root: settings.games_root.trim(),
    servers_root: settings.servers_root.trim(),
    archives_root: settings.archives_root?.trim() || defaultArchiveRoot(settings.servers_root),
    steamcmd_root: settings.steamcmd_root.trim()
  };
}

function comparablePath(path: string): string {
  return path.trim().replace(/\\/g, "/")
    .replace(/^\/\/\?\/([a-z]:\/)/i, "$1")
    .replace(/^\/\/\?\/UNC\//i, "//");
}

function absolutePathParts(path: string) {
  const normalized = comparablePath(path);
  const anchor = normalized.match(/^(?:[a-z]:\/|\/\/[^/]+\/[^/]+(?:\/|$)|\/)/i)?.[0];
  if (!anchor) return null;
  const windows = anchor !== "/";
  const root = anchor.replace(/\/+$/, "");
  const parts: string[] = [];
  for (const part of normalized.slice(anchor.length).split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") { if (!parts.length) return null; parts.pop(); }
    else parts.push(part);
  }
  return { root: windows ? root.toLowerCase() : root, parts, windows };
}

function sameLexicalPath(left: string, right: string): boolean {
  const source = absolutePathParts(left);
  const target = absolutePathParts(right);
  return Boolean(source && target && source.root === target.root && source.parts.length === target.parts.length
    && source.parts.every((part, index) => source.windows
      ? part.toLowerCase() === target.parts[index].toLowerCase() : part === target.parts[index]));
}

type SelectablePath = Exclude<keyof AppPathSettingsInput, "archives_root">;

const managedFolders: Record<SelectablePath, readonly string[]> = {
  games_root: ["server-files"],
  servers_root: ["instances"],
  steamcmd_root: ["cmd", "steamcmd"]
};

export function isStorageVolumeRoot(path: string): boolean {
  const comparable = comparablePath(path);
  return /^[a-z]:\/+$/i.test(comparable) || /^\/\/(?![?.]\/)[^/]+\/[^/]+\/*$/.test(comparable);
}

function resolveSelectedPath(key: SelectablePath, selected: string): string {
  const value = selected.trim();
  if (!isStorageVolumeRoot(value)) return value;
  // A drive/share root selects a location, not the whole directory to replace.
  // Use the same role folders as initial storage setup and keep explicit folders exact.
  const separator = value.includes("\\") ? "\\" : "/";
  return [value.replace(/[\\/]+$/, ""), "LanGame", ...managedFolders[key]].join(separator);
}

export function updateAppPathDraft(
  draft: AppPathSettingsInput,
  key: SelectablePath,
  selected: string
): AppPathSettingsInput {
  if (sameLexicalPath(draft[key], selected)) return { ...draft };
  const value = resolveSelectedPath(key, selected);
  if (sameLexicalPath(draft[key], value)) return { ...draft };
  const next = { ...draft, [key]: value };
  // Archives are owned by the workspace, including previously external archives.
  // Server files and SteamCMD keep their independently configured locations.
  if (key === "servers_root") next.archives_root = defaultArchiveRoot(value);
  return next;
}

export function createAppPathSelection(
  settings: AppPathSettingsInput,
  key: SelectablePath,
  selected: string
): AppPathSettingsInput {
  return updateAppPathDraft(createAppPathSettingsDraft(settings), key, selected);
}

export function createAppPathMigration(
  currentSettings: AppPathSettingsInput,
  input: AppPathSettingsInput
): AppPathSettingsInput {
  const current = createAppPathSettingsDraft(currentSettings);
  const next = createAppPathSettingsDraft(input);
  for (const key of ["games_root", "servers_root", "steamcmd_root"] as const) {
    if (sameLexicalPath(current[key], next[key])) continue;
    const target = resolveSelectedPath(key, next[key]);
    if (target !== next[key]) next[key] = sameLexicalPath(current[key], target) ? current[key] : target;
  }
  // Retry inputs may predate workspace-owned archives. Bind their destination
  // against the current effective workspace without changing the other targets.
  next.archives_root = sameLexicalPath(current.servers_root, next.servers_root)
    ? current.archives_root : defaultArchiveRoot(next.servers_root);
  return next;
}

export function appPathSettingsEqual(left: AppPathSettingsInput, right: AppPathSettingsInput): boolean {
  return left.games_root === right.games_root
    && left.servers_root === right.servers_root
    && left.archives_root === right.archives_root
    && left.steamcmd_root === right.steamcmd_root;
}
