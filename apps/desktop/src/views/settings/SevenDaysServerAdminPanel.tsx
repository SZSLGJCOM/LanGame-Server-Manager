import { useMemo } from "react";
import { useI18n } from "../../i18n";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import type { SettingsObject } from "./settings-schema";

interface CommandPermissionEntry {
  cmd: string;
  permission_level: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function clampPermissionLevel(value: unknown): number {
  const parsed = typeof value === "number"
    ? value
    : typeof value === "string"
      ? Number.parseInt(value, 10)
      : 0;
  return Math.max(0, Math.min(1000, Number.isFinite(parsed) ? Math.trunc(parsed) : 0));
}

function parseCommandPermissions(settings: SettingsObject): CommandPermissionEntry[] {
  if (!Array.isArray(settings.command_permissions)) {
    return [];
  }
  return settings.command_permissions.flatMap((entry) => {
    if (!isRecord(entry)) {
      return [];
    }
    return [{
      cmd: typeof entry.cmd === "string" ? entry.cmd.trim() : "",
      permission_level: clampPermissionLevel(entry.permission_level)
    }];
  });
}

function findDuplicateCommands(entries: CommandPermissionEntry[]): string[] {
  const seen = new Set<string>();
  const duplicates = new Set<string>();
  for (const entry of entries) {
    const normalized = entry.cmd.trim().toLowerCase();
    if (!normalized) {
      continue;
    }
    if (seen.has(normalized)) {
      duplicates.add(entry.cmd.trim());
    } else {
      seen.add(normalized);
    }
  }
  return [...duplicates];
}

export function SevenDaysServerAdminPanel(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const entries = useMemo(() => parseCommandPermissions(props.settings), [props.settings]);
  const duplicates = useMemo(() => findDuplicateCommands(entries), [entries]);
  const editable = !props.disabled;

  const replaceCommandPermissions = (nextEntries: CommandPermissionEntry[]) => {
    props.onPatch({ command_permissions: nextEntries });
  };

  const updateEntry = (index: number, nextEntry: CommandPermissionEntry) => {
    replaceCommandPermissions(entries.map(
      (entry, candidateIndex) => candidateIndex === index ? nextEntry : entry
    ));
  };

  return (
    <section className="settings-schema-section sevendays-admin-shell">
      <div className="settings-schema-section-head">
        <div>
          <h4 className="settings-section-title">
            {t("settings.7dtd.serverAdmin.blocks.commands.title", undefined, "Console command permissions")}
          </h4>
        </div>
        <div className="sevendays-admin-block-toolbar">
          <span className="page-chip">
            {t("settings.7dtd.serverAdmin.rows", { count: entries.length }, "{count} rows")}
          </span>
          <button
            type="button"
            className="secondary-button"
            disabled={!editable}
            onClick={() => replaceCommandPermissions([...entries, { cmd: "", permission_level: 0 }])}
          >
            {t("settings.7dtd.serverAdmin.blocks.commands.add", undefined, "Add command rule")}
          </button>
        </div>
      </div>

      {duplicates.length > 0 ? (
        <div className="sevendays-admin-alerts">
          <article className="sevendays-admin-alert">
            {t(
              "settings.7dtd.serverAdmin.alerts.duplicateCommands",
              { preview: duplicates.join(", ") },
              "Command permissions repeat the same command: {preview}."
            )}
          </article>
        </div>
      ) : null}

      <div className="sevendays-admin-rows">
        {entries.length > 0 ? entries.map((entry, index) => (
          <div key={`command-${index}`} className="sevendays-admin-row">
            <label className="settings-schema-field">
              <span className="detail-label">
                {t("settings.7dtd.serverAdmin.fields.command", undefined, "Command")}
              </span>
              <input
                className="settings-schema-input"
                type="text"
                value={entry.cmd}
                disabled={!editable}
                onChange={(event) => updateEntry(index, { ...entry, cmd: event.target.value.trim() })}
              />
            </label>
            <label className="settings-schema-field">
              <span className="detail-label">
                {t("settings.7dtd.serverAdmin.fields.permissionLevel", undefined, "Permission level")}
              </span>
              <input
                className="settings-schema-input"
                type="number"
                min={0}
                max={1000}
                value={entry.permission_level}
                disabled={!editable}
                onChange={(event) => updateEntry(index, {
                  ...entry,
                  permission_level: clampPermissionLevel(event.target.value)
                })}
              />
            </label>
            <button
              type="button"
              className="ghost-button"
              disabled={!editable}
              onClick={() => replaceCommandPermissions(
                entries.filter((_, candidateIndex) => candidateIndex !== index)
              )}
            >
              {t("settings.7dtd.serverAdmin.remove", undefined, "Remove")}
            </button>
          </div>
        )) : (
          <p className="form-note">
            {t(
              "settings.7dtd.serverAdmin.blocks.commands.empty",
              undefined,
              "No command-specific permissions are configured yet."
            )}
          </p>
        )}
      </div>
    </section>
  );
}
