import type { TranslateFn } from "../../i18n";
import type { SteamWorkshopLookupItem } from "../../types";
import { isClientOnlyDstWorkshopItem, isUnsupportedWorkshopItem, serverWorkshopCollectionChildren } from "./mod-workbench-model";

export type WorkshopInstallationState = "installed" | "partial" | "not-installed" | "checking" | "unknown";
export type WorkshopLifecycleState =
  | "installing"
  | "enabled"
  | "downloaded"
  | "needs-configuration"
  | "partially-configured"
  | "unsupported"
  | "client-only"
  | "pending-download"
  | "partial"
  | "not-installed"
  | "checking"
  | "unknown";

export type WorkshopStoreAction = "install" | "manage";

export interface WorkshopStoreItemState {
  installationState: WorkshopInstallationState;
  lifecycleState: WorkshopLifecycleState;
  configured: boolean;
  enabled: boolean;
  action: WorkshopStoreAction;
}

export function formatWorkshopItemKind(kind: string, t: TranslateFn): string {
  if (kind === "unknown") return t("servers.mods.lookupStatus.unverified");
  if (kind === "guide") return t("servers.mods.storeDetail.guide", undefined, "Steam guide");
  if (kind === "unsupported") return t("servers.mods.storeDetail.unsupported", undefined, "Non-installable content");
  return kind === "collection"
    ? t("servers.mods.storeDetail.collection", undefined, "Collection")
    : t("servers.mods.storeDetail.item", undefined, "Mod");
}

export function formatWorkshopLookupStatus(status: string, t: TranslateFn): string {
  switch (status) {
    case "resolved":
      return t("servers.mods.lookupStatus.resolved", undefined, "Details available");
    case "not_found":
      return t("servers.mods.lookupStatus.notFound", undefined, "Public details unavailable");
    case "pending":
      return t("servers.mods.lifecycle.checking", undefined, "Checking");
    case "unverified":
      return t("servers.mods.lookupStatus.unverified");
    case "unsupported":
      return t("servers.mods.lookupStatus.unsupported", undefined, "Not an installable Mod");
    default:
      return t("servers.mods.lifecycle.unknown", undefined, "Unknown state");
  }
}

export function workshopItemContentIds(item: SteamWorkshopLookupItem, appId: number | null = item.consumer_app_id ?? null): string[] {
  return item.item_kind === "collection" && item.children.length > 0
    ? item.children.filter((child) => child.status === "resolved" && child.item_kind === "item" &&
      !(child.consumer_app_id === appId && isClientOnlyDstWorkshopItem(child, appId))).map((child) => child.id)
    : [item.id];
}

// "installed" means files found in any machine-wide search root, not deployment into this instance.
export function resolveWorkshopInstallationState(
  item: SteamWorkshopLookupItem,
  installedIds: ReadonlySet<string>,
  inspectedIds: ReadonlySet<string>,
  inspectionFailed: boolean,
  expectedAppId: number | null = item.consumer_app_id ?? null
): WorkshopInstallationState {
  const ids = workshopItemContentIds(item, expectedAppId);
  if (ids.length === 0) return "not-installed";
  if (ids.some((id) => !inspectedIds.has(id))) {
    return inspectionFailed ? "unknown" : "checking";
  }

  const installedCount = ids.filter((id) => installedIds.has(id)).length;
  if (installedCount === ids.length) {
    return "installed";
  }
  if (installedCount > 0) {
    return "partial";
  }
  return "not-installed";
}

export function resolveWorkshopLifecycleState(input: {
  installationState: WorkshopInstallationState;
  configured: boolean;
  partiallyConfigured?: boolean;
  enabled: boolean;
  installing: boolean;
  moduleId: string;
}): WorkshopLifecycleState {
  if (input.installing) {
    return "installing";
  }
  if (input.installationState === "checking" || input.installationState === "unknown") {
    return input.installationState;
  }
  if (input.installationState === "partial") {
    return "partial";
  }
  if (input.installationState === "installed") {
    if (input.enabled) {
      return "enabled";
    }
    if (input.partiallyConfigured) {
      return "partially-configured";
    }
    if (input.moduleId === "projectzomboid" && input.configured) {
      return "needs-configuration";
    }
    return "downloaded";
  }
  if (input.configured) {
    return "pending-download";
  }
  return "not-installed";
}

export function resolveWorkshopStoreItemState(input: {
  item: SteamWorkshopLookupItem;
  moduleId: string;
  expectedAppId: number | null;
  configuredIds: ReadonlySet<string>;
  cachedIds: ReadonlySet<string>;
  inspectedIds: ReadonlySet<string>;
  inspectionFailed: boolean;
  installingIds: ReadonlySet<string>;
  steamDownloadMode?: "steamcmd-cache" | "not-wired";
  enabled?: boolean;
}): WorkshopStoreItemState {
  const contentIds = workshopItemContentIds(input.item, input.expectedAppId);
  const configuredCount = contentIds.filter((id) => input.configuredIds.has(id)).length;
  // Collection ownership does not prove that all of its child Mods are enabled.
  const configured = contentIds.length > 0 && configuredCount === contentIds.length;
  const partiallyConfigured = configuredCount > 0 && configuredCount < contentIds.length;
  const enabled = configured && (input.enabled ?? true);
  const installationState = resolveWorkshopInstallationState(
    input.item, input.cachedIds, input.inspectedIds, input.inspectionFailed, input.expectedAppId
  );
  const installing = input.installingIds.has(input.item.id) || contentIds.some((id) => input.installingIds.has(id));
  const clientOnlyCollection = input.item.item_kind === "collection" && input.item.children.length > 0 &&
    serverWorkshopCollectionChildren(input.item, {}, input.expectedAppId)?.length === 0;
  const lifecycleState = isClientOnlyDstWorkshopItem(input.item, input.expectedAppId) || clientOnlyCollection ? "client-only"
    : isUnsupportedWorkshopItem(input.item, input.expectedAppId) ? "unsupported"
    : resolveWorkshopLifecycleState({
      installationState, configured, partiallyConfigured, enabled, installing, moduleId: input.moduleId
    });
  const downloadRequired = input.steamDownloadMode === "steamcmd-cache" &&
    (installationState === "not-installed" || installationState === "partial");
  return { installationState, lifecycleState, configured, enabled, action: downloadRequired || !enabled ? "install" : "manage" };
}

export function workshopDescriptionText(value: string | null | undefined): string {
  if (!value?.trim()) {
    return "";
  }

  const withoutMedia = value
    .replace(/\[img\][\s\S]*?\[\/img\]/gi, "")
    .replace(/\[previewimg[^\]]*\][\s\S]*?\[\/previewimg\]/gi, "")
    .replace(/\[url=[^\]]+\]([\s\S]*?)\[\/url\]/gi, "$1")
    .replace(/\[\*\]\s*/gi, "• ")
    .replace(/\[\/?[a-z][^\]]*\]/gi, "")
    .replace(/&amp;/gi, "&")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&quot;/gi, "\"")
    .replace(/&#39;/gi, "'")
    .replace(/\r\n?/g, "\n");

  const lines = withoutMedia.split("\n").map((line) => line.trim());
  const normalized: string[] = [];
  for (const line of lines) {
    if (!line && normalized[normalized.length - 1] === "") {
      continue;
    }
    normalized.push(line);
  }
  return normalized.join("\n").trim();
}

export function formatWorkshopByteSize(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    return "";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = value;
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  const precision = unitIndex === 0 || amount >= 100 ? 0 : amount >= 10 ? 1 : 2;
  return `${amount.toFixed(precision)} ${units[unitIndex]}`;
}
