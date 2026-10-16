import catalogue from "./guides.json";
import type { LocaleCode } from "../i18n-config";

export type MigrationStatus = "direct" | "conditional" | "unsupported" | "unverified" | "not_applicable";
export interface LocalizedSaveText { zh: string; en: string }
export interface GameSaveImportGuide {
  moduleId: string;
  server: LocalizedSaveText & { status: MigrationStatus };
  singlePlayer: LocalizedSaveText & { status: MigrationStatus };
  source: LocalizedSaveText;
  cautions: LocalizedSaveText[];
  references: { label: string; url: string }[];
}

function status(value: string): MigrationStatus {
  switch (value) {
    case "direct": case "conditional": case "unsupported": case "unverified": case "not_applicable": return value;
    default: throw new Error(`Unknown save migration status: ${value}`);
  }
}

export const SAVE_IMPORT_GUIDES: readonly GameSaveImportGuide[] = catalogue.map(guide => ({
  ...guide, server: { ...guide.server, status: status(guide.server.status) },
  singlePlayer: { ...guide.singlePlayer, status: status(guide.singlePlayer.status) }
}));

export function findSaveImportGuide(moduleId: string): GameSaveImportGuide | undefined {
  return SAVE_IMPORT_GUIDES.find(guide => guide.moduleId === moduleId);
}

export function saveGuideText(value: LocalizedSaveText, locale: LocaleCode): string {
  return locale === "zh-CN" ? value.zh : value.en;
}

export function migrationStatusText(value: MigrationStatus, locale: LocaleCode): string {
  const labels = {
    direct: { zh: "可迁移", en: "Transferable" },
    conditional: { zh: "需满足条件", en: "Conditional" },
    unsupported: { zh: "不支持", en: "Unsupported" },
    unverified: { zh: "尚未证实", en: "Unverified" },
    not_applicable: { zh: "不适用", en: "Not applicable" }
  };
  return saveGuideText(labels[value], locale);
}
