import { useState, type FormEvent } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { formatDesktopError } from "../../desktop-error-message";
import { useI18n } from "../../i18n";
import { isTerrariaModName, readTerrariaEnabledModNames } from "./mod-workbench-terraria";

interface Props {
  disabled: boolean;
  onAdd: (names: string[]) => Promise<void>;
}

/** Accept a saved modpack or the internal names of locally installed Mods. */
export function TerrariaEnabledNameImport({ disabled, onAdd }: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState<{ error: boolean; text: string } | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (disabled || !draft.trim()) return;
    const input = draft.trim();
    let names: string[];
    if (input.startsWith("[")) {
      let parsed: unknown;
      try { parsed = JSON.parse(input); } catch { parsed = null; }
      if (!Array.isArray(parsed) || !parsed.length || !parsed.every((value) =>
        typeof value === "string" && value.trim() && !/[\r\n]/.test(value))) {
        setMessage({ error: true, text: t("servers.mods.terraria.invalidNameList") });
        return;
      }
      names = parsed.map((value: string) => value.trim());
    } else {
      names = readTerrariaEnabledModNames(input);
    }
    if (!names.length || !names.every(isTerrariaModName)) {
      setMessage({ error: true, text: t("servers.mods.terraria.invalidNameList") });
      return;
    }
    try {
      await onAdd(names);
      setDraft("");
      setMessage({ error: false, text: t("servers.mods.terraria.namesSaved") });
    } catch (error) {
      setMessage({ error: true, text: formatDesktopError(t, error) });
    }
  }

  return <details className="mw-terraria-import">
      <summary>{t("servers.mods.terraria.importNames")}</summary>
      <form onSubmit={submit}>
        <label>
          {t("servers.mods.terraria.nameListHelp")}
          <textarea className="mw-textarea" rows={3} maxLength={65536} value={draft} disabled={disabled}
            onChange={(event) => { setDraft(event.target.value); setMessage(null); }} />
        </label>
        <button className="mw-btn mw-btn--secondary" type="submit" disabled={disabled || !draft.trim()}>
          {t("servers.mods.terraria.addNames")}
        </button>
        {message ? <ActivityNotice tone={message.error ? "error" : "success"}>{message.text}</ActivityNotice> : null}
      </form>
  </details>;
}
