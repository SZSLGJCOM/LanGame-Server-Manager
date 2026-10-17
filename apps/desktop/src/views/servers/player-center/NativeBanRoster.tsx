import { useEffect, useRef, useState } from "react";
import { executeDeclaredRuntimePlayerAction, unbanInstanceNativeBan } from "../../../api";
import { describeError } from "../../../app-state";
import { ActivityNotice } from "../../../components/ActivityNotice";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { ShellIcon } from "../../../components/ShellIcon";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { NativeBanRoster } from "../../../types";
import type { NativeBanRosterState } from "./use-native-ban-roster";

type BanEntry = NativeBanRoster["entries"][number];

export function NativeBanRosterList(props: {
  state: NativeBanRosterState;
  selectedAccountId: string | null;
  disabled: boolean;
  locale: LocaleCode;
  onSelect: (accountId: string) => void;
}) {
  const { state, locale } = props;
  return <div className={`player-access-roster-list native-ban-roster${state.moduleId === "runescapedragonwilds" ? " dragonwilds-ban-roster" : ""}`} aria-busy={state.loading}>
    {state.error ? <ActivityNotice tone="error">{selectLocaleText(locale,
      `封禁名单读取失败：${state.error}。现有记录仅供查看，请刷新后再解封。`,
      `Could not read the ban roster: ${state.error}. Existing entries are for inspection; refresh before unbanning.`)}</ActivityNotice> : null}
    <div className="player-access-roster-entries" role="list" aria-label={selectLocaleText(locale, "被封禁玩家", "Banned players")}>
      {state.roster?.entries.map((entry) => <div key={entry.account_id} role="listitem"
        className={`player-access-roster-entry${props.selectedAccountId === entry.account_id ? " is-selected" : ""}`}>
        <button type="button" className="player-center-player-select player-access-roster-select"
          data-banned-account={entry.account_id} aria-pressed={props.selectedAccountId === entry.account_id}
          disabled={props.disabled} onClick={() => props.onSelect(entry.account_id)}
          aria-label={selectLocaleText(locale, `选择被封禁玩家 ${entry.display_name ?? entry.account_id} (${entry.account_id})`,
            `Select banned player ${entry.display_name ?? entry.account_id} (${entry.account_id})`)}>
          <span className="server-player-name">{entry.display_name ?? entry.account_id}</span>
          {entry.display_name ? <span className="player-access-roster-entry-value" title={entry.account_id}>{entry.account_id}</span> : null}
        </button>
      </div>)}
      {state.loading && !state.roster ? <div className="player-access-roster-empty" role="status"><ShellIcon name="refresh" className="player-access-roster-empty-icon is-spinning" />
        <span>{selectLocaleText(locale, "正在读取封禁名单", "Loading ban roster")}</span></div> : null}
      {state.roster && !state.loading && !state.stale && !state.error && state.roster.entries.length === 0
        ? <div className="player-access-roster-empty" role="status"><ShellIcon name="inbox" className="player-access-roster-empty-icon" />
          <span>{selectLocaleText(locale, "暂无被封禁玩家", "No banned players")}</span></div> : null}
    </div>
  </div>;
}

export function NativeBanRosterActions(props: {
  instanceId: string;
  moduleId: string;
  entry: BanEntry | null;
  /** Retains the module's pre-existing explicit manual-ID action independently of file discovery. */
  manualUnban?: { target: string; actionId: string; error: string | null };
  hasSelection?: boolean;
  state: NativeBanRosterState;
  readOnly: boolean;
  running: boolean;
  locale: LocaleCode;
  onBusyChange: (busy: boolean) => void;
  onCompleted: () => Promise<void>;
}) {
  const scopeRef = useRef<object | null>(null);
  const inFlightRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ accountId: string; tone: "success" | "warning" | "error"; text: string } | null>(null);
  useEffect(() => {
    const scope = {};
    scopeRef.current = scope;
    return () => { if (scopeRef.current === scope) scopeRef.current = null; };
  }, [props.instanceId, props.moduleId]);
  const reason = busy ? selectLocaleText(props.locale, "正在解封，请等待完成。", "Unbanning. Wait for it to finish.")
    : props.readOnly ? selectLocaleText(props.locale, "当前处于只读模式，无法解封。", "This workspace is read-only; unbanning is unavailable.")
      : !props.running && props.moduleId !== "squad" ? selectLocaleText(props.locale, "服务器未运行，启动后可解封。", "Server not running; start it to unban.")
        : props.entry?.identity_kind === "palworld_player_uid" ? selectLocaleText(props.locale,
          "这条旧封禁只有玩家 GUID，缺少原生用户 ID；当前接口无法解封。",
          "This historical ban has only a player GUID and no native user ID; the current interface cannot unban it.")
        : props.manualUnban ? props.manualUnban.error
        : props.state.loading ? selectLocaleText(props.locale, "封禁名单正在读取，请等待完成。", "Wait for the ban roster to finish loading.")
          : props.state.stale || props.state.error || !props.state.roster ? selectLocaleText(props.locale,
            "请先成功刷新封禁名单，再执行解封。", "Refresh the ban roster successfully before unbanning.")
            : !props.entry ? props.hasSelection
              ? selectLocaleText(props.locale, "所选账号不在封禁名单中。", "The selected account is not in the ban roster.")
              : selectLocaleText(props.locale, "请先选择玩家或名单条目。", "Select a player or roster entry first.") : null;
  async function unban() {
    const entry = props.entry;
    const manualUnban = props.manualUnban;
    const accountId = manualUnban?.target ?? entry?.account_id;
    const scope = scopeRef.current;
    if (!accountId || !scope || reason || inFlightRef.current) return;
    inFlightRef.current = true;
    setBusy(true);
    props.onBusyChange(true);
    setFeedback(null);
    try {
      if (manualUnban || props.moduleId === "runescapedragonwilds") {
        await executeDeclaredRuntimePlayerAction(props.instanceId, manualUnban?.actionId ?? "unban_player", accountId, "");
      } else {
        await unbanInstanceNativeBan(props.instanceId, accountId);
      }
      if (scopeRef.current !== scope) return;
      try {
        await props.onCompleted();
        if (scopeRef.current === scope) {
          if (props.moduleId === "squad") {
            const current = props.state.getFreshRoster();
            if (!current || current.entries.some((candidate) => candidate.account_id === accountId)) {
              setFeedback({ accountId, tone: "warning", text: selectLocaleText(props.locale,
                `封禁文件仍有此账号或读取结果未确认；请刷新核对。持久变更须重启后生效：${accountId}`,
                `The account is still in the saved bans or the result is unconfirmed. Refresh to check; persisted changes require a restart: ${accountId}`) });
              return;
            }
            setFeedback({ accountId, tone: "warning", text: selectLocaleText(props.locale,
              props.running ? `持久封禁已移除；服务器重启后生效，当前运行中的封禁缓存尚未解除：${accountId}`
                : `持久封禁已移除；下次启动服务器时生效：${accountId}`,
              props.running ? `Saved ban removed. Restart the server to update its local ban cache: ${accountId}`
                : `Saved ban removed. This takes effect on the next server start: ${accountId}`) });
            return;
          }
          if (manualUnban) {
            setFeedback({ accountId, tone: "success", text: selectLocaleText(props.locale,
              `解封命令已发送，已刷新封禁列表：${accountId}`, `Unban command sent; the ban roster was refreshed: ${accountId}`) });
            return;
          }
          const refreshed = props.state.getFreshRoster();
          const confirmed = refreshed && !refreshed.entries.some((candidate) => candidate.identity_kind === entry?.identity_kind && candidate.account_id === accountId);
          setFeedback({ accountId, tone: confirmed ? "success" : "warning", text: confirmed
            ? selectLocaleText(props.locale, `封禁列表已确认解除封禁：${accountId}`, `Ban roster confirms unban: ${accountId}`)
            : selectLocaleText(props.locale, `解封命令已发送，封禁列表仍有此账号；结果尚未确认，请刷新核对：${accountId}`,
              `Unban command sent, but the ban roster still contains this account. Result unconfirmed; refresh to check: ${accountId}`) });
        }
      } catch (cause) {
        if (scopeRef.current === scope) setFeedback({ accountId, tone: "warning",
          text: props.moduleId === "squad" ? selectLocaleText(props.locale,
            `持久封禁删除已执行，但最终名单未确认；持久变更须重启后生效。刷新失败：${describeError(cause)}`,
            `Saved ban removal executed, but the final roster is unconfirmed. Persisted changes require a restart. Refresh failed: ${describeError(cause)}`)
            : props.moduleId === "runescapedragonwilds"
            ? selectLocaleText(props.locale, `解封已确认，但刷新失败：${describeError(cause)}`, `Unban confirmed, but refresh failed: ${describeError(cause)}`)
            : selectLocaleText(props.locale, `解封命令已发送，但刷新失败；结果尚未确认：${describeError(cause)}`,
              `Unban command sent, but refresh failed; result unconfirmed: ${describeError(cause)}`) });
      }
    } catch (cause) {
      if (scopeRef.current === scope) {
        props.state.invalidate();
        setFeedback({ accountId, tone: "error", text: describeError(cause) });
      }
    } finally {
      inFlightRef.current = false;
      if (scopeRef.current === scope) { setBusy(false); props.onBusyChange(false); }
    }
  }
  const selectedAccountId = props.manualUnban?.target ?? props.entry?.account_id;
  const visibleFeedback = feedback && (!selectedAccountId || feedback.accountId === selectedAccountId) ? feedback : null;
  const identityLabel = props.entry?.identity_kind === "eos_id" ? "EOS " : props.entry?.identity_kind === "steam_id" ? "Steam " : "";
  return <section className={`player-center-member-pane native-ban-actions${props.moduleId === "runescapedragonwilds" ? " dragonwilds-ban-actions" : ""}`} aria-busy={busy}
    aria-label={selectLocaleText(props.locale, "封禁名单操作", "Ban roster actions")}>
    <section className="player-center-action-group"><div className="player-center-action-list">
      {/* The unavailable reason is the button's tooltip, like every other stack button. */}
      <ConfigurationHelp description={reason}>{(help) => <div className="player-access-total-action" ref={help.anchorRef} {...help.interactionProps}
        tabIndex={reason ? 0 : undefined} role={reason ? "group" : undefined} aria-disabled={Boolean(reason) || undefined}
        aria-describedby={help.descriptionId}>
      <InlineConfirmAction className={`secondary-button player-center-action-button native-unban-action${props.moduleId === "runescapedragonwilds" ? " dragonwilds-unban-action" : ""}`} disabled={Boolean(reason)} aria-describedby={help.descriptionId}
        scopeKey={JSON.stringify([props.instanceId, props.moduleId, props.manualUnban?.actionId, props.entry?.identity_kind, selectedAccountId, props.state.roster?.observed_at_unix_ms])}
        confirmation={props.moduleId === "squad" ? selectLocaleText(props.locale,
          `移除账号「${selectedAccountId ?? ""}」的持久封禁？重启后生效。`,
          `Remove the saved ban for “${selectedAccountId ?? ""}”? Takes effect after restart.`)
          : selectLocaleText(props.locale, `确认解除 ${identityLabel}账号「${selectedAccountId ?? ""}」的封禁？`,
            `Unban ${identityLabel}account “${selectedAccountId ?? ""}”?`)} onConfirm={unban}>
        {selectLocaleText(props.locale, "解除封禁", "Unban")}
      </InlineConfirmAction>
      </div>}</ConfigurationHelp>
    </div>
    {props.moduleId === "squad" ? <p className="player-center-detail-note">{selectLocaleText(props.locale,
      "移除持久封禁后，服务器重启时生效。", "Removing a saved ban takes effect after the server restarts.")}</p> : null}
    </section>
    {visibleFeedback ? <ActivityNotice tone={visibleFeedback.tone}>{visibleFeedback.text}</ActivityNotice> : null}
  </section>;
}
