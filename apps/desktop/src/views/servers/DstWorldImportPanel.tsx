import { ActivityNotice } from "../../components/ActivityNotice";
import { useId, useRef, useState } from "react";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { DstWorldImportResult, InstanceDetails } from "../../types";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";

interface DstWorldImportPanelProps {
  details: InstanceDetails;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  onImportWorldData: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

export function DstWorldImportPanel(props: DstWorldImportPanelProps) {
  const { locale: languageTag, t } = useI18n();
  const sourceHelpId = useId();
  const [selectedSourcePath, setSelectedSourcePath] = useState<string | null>(null);
  const [importResult, setImportResult] = useState<DstWorldImportResult | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  const [pickBusy, setPickBusy] = useState(false);
  const operationBusy = useRef(false);

  const running = Boolean(props.details.active_run);
  const copy = {
    importBlockedChip: t("dst.settings.bootstrap.importBlockedChip", undefined, "Stop the server first"),
    importBusyChip: t("dst.settings.bootstrap.importBusyChip", undefined, "Importing"),
    importTitle: t("dst.settings.bootstrap.importTitle", undefined, "Import save"),
    importBody: t("dst.settings.bootstrap.importBody", undefined, "Choose the cluster folder with an overworld (Master) save and the other shards required by its layout: Caves, or Caves, Islands and Volcano."),
    sourceLabel: t("dst.settings.bootstrap.sourceLabel", undefined, "Source folder"),
    sourcePlaceholder: t("dst.settings.bootstrap.sourcePlaceholder", undefined, "No folder selected yet"),
    safeguardLabel: t("dst.settings.bootstrap.safeguardLabel", undefined, "Recoverable pre-import backup"),
    pickButton: t("dst.settings.bootstrap.pickButton", undefined, "Choose folder"),
    importButton: t("dst.settings.bootstrap.importButton", undefined, "Import into this instance"),
    importingButton: t("dst.settings.bootstrap.importingButton", undefined, "Importing..."),
    hint: t("dst.settings.bootstrap.hint", undefined, "Replaces the current save after creating a backup. Restores the source world's Mod enablement and options, then downloads missing Mod files before starting."),
    importSummaryTitle: t("dst.settings.bootstrap.importSummaryTitle", undefined, "Latest import"),
    importedMaster: t("dst.settings.bootstrap.importedMaster", undefined, "Overworld imported"),
    importedCaves: t("dst.settings.bootstrap.importedCaves", undefined, "Caves imported"),
    importedIslands: t("dst.settings.bootstrap.importedIslands", undefined, "Islands imported"),
    importedVolcano: t("dst.settings.bootstrap.importedVolcano", undefined, "Volcano imported"),
    importedMods: (count: number) => t("dst.settings.bootstrap.importedMods", { count }, "Restored {count} Workshop Mods"),
    importedFilesLabel: t("dst.settings.bootstrap.importedFilesLabel", undefined, "Files copied"),
    noShards: t("dst.settings.bootstrap.noShards", undefined, "No recognized shard data found"),
    selectFolderFirst: t("dst.settings.bootstrap.selectFolderFirst", undefined, "Select a save folder first.")
  };
  const sourceHelp = useConfigurationFieldHelp(useId(), selectedSourcePath,
    undefined, undefined, "instructions");
  const importHelp = useConfigurationFieldHelp(sourceHelpId, copy.importBody,
    undefined, undefined, "instructions");

  async function handlePickFolder() {
    if (operationBusy.current) return;
    operationBusy.current = true;
    setPickBusy(true);
    try {
      const nextPath = await props.onPickDirectory(selectedSourcePath);
      if (!nextPath) return;
      setSelectedSourcePath(nextPath);
      setImportResult(null);
      setImportError(null);
    } catch (error) {
      setImportError(t(
        "dst.settings.bootstrap.pickFailed",
        { message: describeError(error) },
        "Could not choose a save folder: {message}"
      ));
    } finally {
      operationBusy.current = false;
      setPickBusy(false);
    }
  }

  async function handleImport() {
    if (running || operationBusy.current) return;
    if (!selectedSourcePath) {
      setImportError(copy.selectFolderFirst);
      return;
    }

    operationBusy.current = true;
    setImportBusy(true);
    setImportResult(null);
    setImportError(null);
    try {
      const result = await props.onImportWorldData(props.details.summary.id, selectedSourcePath);
      setImportResult(result);
    } catch (error) {
      setImportError(t(
        "dst.settings.bootstrap.importFailed",
        { message: describeError(error) },
        "World import failed: {message}"
      ));
    } finally {
      operationBusy.current = false;
      setImportBusy(false);
    }
  }

  const shardLabels: Record<string, string> = { Master: copy.importedMaster, Caves: copy.importedCaves,
    Islands: copy.importedIslands, Volcano: copy.importedVolcano };
  const importedShards = importResult?.imported_shards ?? [
    importResult?.imported_master ? "Master" : null, importResult?.imported_caves ? "Caves" : null
  ].filter((shard): shard is string => shard !== null);
  const shardSummary = importedShards.map((shard) => shardLabels[shard] ?? shard).join(" · ");

  return (
    <div className="dst-world-import">
      <div className="server-workbench-section-label dst-world-import-header">
        <ShellIcon name="folder" className="server-workbench-section-icon" />
        <span>{copy.importTitle}</span>
        {importBusy || running ? <span className="status-chip is-busy" role={importBusy ? "status" : undefined}>
          {importBusy ? copy.importBusyChip : copy.importBlockedChip}
        </span> : null}
      </div>
      <div className="dst-world-import-form" aria-busy={importBusy || pickBusy}>
        {importHelp.helpNode}
        <div className="dst-world-import-source-row">
          <span className="dst-world-import-source-path" ref={sourceHelp.anchorRef} {...sourceHelp.interactionProps}
            tabIndex={sourceHelp.descriptionId ? 0 : undefined} aria-describedby={sourceHelp.descriptionId}
            aria-label={`${copy.sourceLabel}: ${selectedSourcePath ?? copy.sourcePlaceholder}`}>
            {selectedSourcePath ?? copy.sourcePlaceholder}
          </span>
          {sourceHelp.helpNode}
          <button type="button" className="secondary-button" ref={importHelp.anchorRef} {...importHelp.interactionProps}
            aria-describedby={importHelp.descriptionId}
            onClick={() => void handlePickFolder()} disabled={importBusy || pickBusy}>
            {copy.pickButton}
          </button>
        </div>
        <div className="dst-world-import-actions">
          <p className="form-note">{copy.hint}</p>
          <button
            type="button"
            className="primary-button"
            onClick={() => void handleImport()}
            disabled={importBusy || pickBusy || running || !selectedSourcePath}
          >
            {importBusy ? copy.importingButton : copy.importButton}
          </button>
        </div>
      </div>

      {importError ? <ActivityNotice tone="error">{importError}</ActivityNotice> : null}

      {importResult ? (
        <div className="dst-world-import-feedback is-success" role="status">
          <div>{copy.importSummaryTitle}</div>
          <div>{shardSummary || copy.noShards}</div>
          <div>{copy.importedMods(importResult.imported_workshop_mod_ids?.length ?? 0)}</div>
          <div>
            {copy.importedFilesLabel}: {importResult.copied_file_count} · {formatBytes(languageTag, importResult.copied_total_bytes)}
          </div>
          <div className="detail-label">{copy.safeguardLabel}</div>
          <div className="detail-value detail-value--code">{importResult.safeguard_path}</div>
        </div>
      ) : null}
    </div>
  );
}

function formatBytes(languageTag: string, value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return "0 B";
  }

  const unitIndex = Math.min(Math.floor(Math.log(value) / Math.log(1024)), BYTE_UNITS.length - 1);
  const normalized = value / 1024 ** unitIndex;
  const maximumFractionDigits = normalized >= 100 ? 0 : normalized >= 10 ? 1 : 2;
  return `${new Intl.NumberFormat(languageTag, { maximumFractionDigits }).format(normalized)} ${BYTE_UNITS[unitIndex]}`;
}
