import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { formatDesktopError } from "../../desktop-error-message";
import { useI18n } from "../../i18n";
import type { SteamWorkshopAccountState } from "./useSteamWorkshopAccount";
import "./steam-workshop-account.css";

export function SteamWorkshopAccountConsole({ account, onShowServer, targetLabel, active = true }: {
  account: SteamWorkshopAccountState; onShowServer?: () => void; targetLabel?: string; active?: boolean
}) {
  const { t } = useI18n();
  const [inputDraft, setInputDraft] = useState({ scope: "", value: "" });
  const terminal = useRef<HTMLPreElement>(null), input = useRef<HTMLInputElement>(null);
  const snapshot = account.snapshot;
  const inputScope = `${snapshot?.session_id}:${snapshot?.revision}:${snapshot?.input_mode}`;
  const draft = inputDraft.scope === inputScope ? inputDraft.value : "";
  const secret = snapshot?.input_mode === "secret";
  const guardCode = snapshot?.phase === "guard_code" && secret;
  const canInput = Boolean(snapshot && snapshot.input_mode !== "disabled" && !account.sending && !account.cancelling);
  const terminalPhase = snapshot && ["authenticated", "failed", "cancelled"].includes(snapshot.phase);
  const statusPhase = account.cancelling ? "cancelling" : snapshot?.phase ?? (account.starting ? "starting_session"
    : account.error || account.inputError ? "failed" : "anonymous");
  const processing = ["starting_session", "input_submitted", "queued", "preparing_steamcmd", "connecting", "authenticating", "verifying_cache", "cancelling"].includes(statusPhase);
  const canRestart = !account.busy && (!snapshot || terminalPhase);
  const events = snapshot?.events.slice(-120) ?? [];
  const transcript = events.length ? events.map((phase) => t(`servers.mods.steamAccount.phase.${phase}`, {
    account: snapshot?.account_name ?? ""
  })).join("\n") : t(account.inputError || account.error ? "servers.mods.steamAccount.phase.failed"
    : account.starting ? "servers.mods.steamAccount.phase.starting_session" : "servers.mods.steamAccount.anonymousPrompt");
  // A prompt transition must never render a previous secret in a text input.
  useLayoutEffect(() => { setInputDraft({ scope: inputScope, value: "" }); }, [inputScope]);
  useLayoutEffect(() => { if (active && terminal.current) terminal.current.scrollTop = terminal.current.scrollHeight; }, [transcript, active]);
  useEffect(() => { if (active && canInput) input.current?.focus(); }, [canInput, snapshot?.input_mode, active]);

  return <section className="detail-stack server-module-stack" hidden={!active}
    style={!active ? { display: "none" } : undefined}>
    <article className="runtime-box workbench-card workbench-card--wide server-runtime-surface-card server-runtime-console-card server-steamcmd-console">
      <div className="server-runtime-console-frame">
        <div className="server-runtime-console-toolbar server-runtime-console-toolbar--primary">
          <div className="server-runtime-console-title"><span title={targetLabel}>LanGameCMD · SteamCMD{targetLabel ? ` · ${targetLabel}` : ""}</span>
            <span className={`server-steamcmd-status${processing ? " server-steamcmd-status--processing" : ""}`}
              data-phase={statusPhase} role="status" aria-live="polite">{t(`servers.mods.steamAccount.status.${statusPhase}`)}</span>
          </div>
          <div className="server-runtime-terminal-actions">
            <select className="server-runtime-console-target-select server-steamcmd-identity" aria-label={t("servers.mods.steamAccount.mode")}
              value={account.loginAccount ?? ""} disabled={account.busy || account.recoveryPending}
              onChange={(event) => account.selectAccount(event.target.value || null)}>
              <option value="">{t("servers.mods.steamAccount.anonymous")}</option>
              {account.authenticatedAccount ? <option value={account.authenticatedAccount}>{account.authenticatedAccount}</option> : null}
            </select>
            {account.target?.kind === "instance" && onShowServer ? <select className="server-runtime-console-target-select server-steamcmd-source" value="steamcmd"
              aria-label={t("servers.mods.steamAccount.consoleSource")} onChange={() => onShowServer()}>
              <option value="server">{t("servers.mods.steamAccount.serverConsole")}</option><option value="steamcmd">SteamCMD</option>
            </select> : null}
            {canRestart ? <button className="mw-ghost-btn" type="button" onClick={() => void account.start()}>
              <ShellIcon name="user" className="mw-btn-icon" />{t(snapshot?.phase === "authenticated" ? "servers.mods.steamAccount.loginAnother"
                : !snapshot && !account.inputError && !account.error ? "servers.mods.steamAccount.login" : "common.retry")}</button>
              : <button className="mw-ghost-btn" type="button" disabled={!snapshot || account.cancelling || account.selecting} onClick={() => void account.cancel()}>
                {t(account.cancelling ? "servers.mods.steamAccount.cancelling" : "common.cancel")}</button>}
            {canRestart && account.recoveryPending ? <button className="mw-ghost-btn" type="button" onClick={() => void account.cancel()}>{t("common.cancel")}</button> : null}
          </div>
        </div>
        <pre ref={terminal} className="log-preview server-log-preview server-runtime-console server-runtime-console--primary"
          onCopy={(event) => event.preventDefault()}>
          {account.recoveryPending ? `${t(account.recoveryPurpose === "server-install"
            ? "servers.mods.steamAccount.serverAccessRequired" : "servers.mods.steamAccount.accessRequired")}\n\n` : ""}
          {transcript}
          {account.inputError ? <span role="alert" className="server-steamcmd-input-error">{`\n\n${t(`servers.mods.steamAccount.inputError.${account.inputError}`)}`}</span> : null}
          {account.error ? <span role="alert" className="server-runtime-console-errors">{`\n\n${formatDesktopError(t, account.error)}`}</span> : null}
          {snapshot?.error ? <span role="alert" className="server-runtime-console-errors">
            {`\n\n${formatDesktopError(t, snapshot.error)}`}</span> : null}
        </pre>
        <form className="server-runtime-console-command-form" aria-label={t("servers.mods.steamAccount.inputLabel")}
          onSubmit={(event) => {
            event.preventDefault();
            if (!canInput || !draft.length) return;
            const value = secret ? draft : draft.trim();
            setInputDraft({ scope: inputScope, value: "" });
            if (value.length && snapshot) void account.send(value, snapshot);
          }}>
          <span className="server-runtime-console-prompt" aria-hidden="true">SteamCMD&gt;</span>
          <input ref={input} type={secret ? "password" : "text"} className="server-runtime-console-command-input"
            value={draft} autoComplete="off" autoCapitalize="none" autoCorrect="off" spellCheck={false} maxLength={1024}
            aria-label={t(guardCode ? "servers.mods.steamAccount.tokenPrompt"
              : snapshot?.phase === "email_code" ? "servers.mods.steamAccount.emailPrompt"
              : snapshot?.phase === "password" ? "servers.mods.steamAccount.passwordPrompt"
              : snapshot?.phase === "steam_guard" ? "servers.mods.steamAccount.guardPrompt" : "servers.mods.steamAccount.inputLabel")} disabled={!canInput}
            placeholder={t(!canInput ? `servers.mods.steamAccount.status.${statusPhase}` : guardCode ? "servers.mods.steamAccount.tokenPrompt"
              : snapshot?.phase === "email_code" ? "servers.mods.steamAccount.emailPrompt"
              : snapshot?.phase === "password" ? "servers.mods.steamAccount.passwordPrompt"
              : snapshot?.phase === "steam_guard" ? "servers.mods.steamAccount.guardPrompt"
              : "servers.mods.steamAccount.commandPrompt")}
            onCopy={(event) => { if (secret) event.preventDefault(); }}
            onCut={(event) => { if (secret) event.preventDefault(); }}
            onChange={(event) => setInputDraft({ scope: inputScope, value: event.target.value })} />
        </form>
      </div>
    </article>
  </section>;
}
