import { useEffect, useRef, useState } from "react";
import { executeDeclaredRuntimePlayerAction } from "../../../api";
import { describeError } from "../../../app-state";
import { ActivityNotice } from "../../../components/ActivityNotice";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { ShellIcon } from "../../../components/ShellIcon";
import { selectLocaleText, type LocaleCode } from "../../../i18n";
import type { DragonwildsBanRoster } from "../../../types";
import type { DragonwildsBanRosterState } from "./use-dragonwilds-ban-roster";

type BanEntry = DragonwildsBanRoster["entries"][number];

export function DragonwildsBanRosterList(props: {
  state: DragonwildsBanRosterState;
  selectedAccountId: string | null;
  disabled: boolean;
  locale: LocaleCode;
  onSelect: (accountId: string) => void;
}) {
  const { state, locale } = props;
  return <div className="player-access-roster-list dragonwilds-ban-roster" aria-busy={state.loading}>
    {state.loading ? <p className="form-note" role="status">{selectLocaleText(locale,
      state.roster ? "正在刷新封禁名单…" : "正在读取封禁名单…",
      state.roster ? "Refreshing ban roster…" : "Loading ban roster…")}</p> : null}
    {state.error ? <ActivityNotice tone="error">{selectLocaleText(locale,
      `封禁名单读取失败：${state.error}。现有记录仅供查看，请刷新后再解封。`,
      `Could not read the ban roster: ${state.error}. Existing entries are for inspection; refresh before unbanning.`)}</ActivityNotice>
      : state.stale && !state.loading && state.roster ? <p className="form-note" role="status">{selectLocaleText(locale,
        "封禁名单可能已更改，请刷新后再解封。", "The ban roster may have changed. Refresh before unbanning.")}</p> : null}
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
      {state.roster && !state.loading && !state.stale && !state.error && state.roster.entries.length === 0
        ? <div className="player-access-roster-empty" role="status"><ShellIcon name="inbox" className="player-access-roster-empty-icon" />
          <span>{selectLocaleText(locale, "暂无被封禁玩家", "No banned players")}</span></div> : null}
    </div>
  </div>;
}

export function DragonwildsBanRosterActions(props: {
  instanceId: string;
  entry: BanEntry | null;
  hasSelection?: boolean;
  state: DragonwildsBanRosterState;
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
  }, [props.instanceId]);
  const reason = busy ? selectLocaleText(props.locale, "正在解封，请等待完成。", "Unbanning. Wait for it to finish.")
    : props.readOnly ? selectLocaleText(props.locale, "当前处于只读模式，无法解封。", "This workspace is read-only; unbanning is unavailable.")
      : !props.running ? selectLocaleText(props.locale, "启动服务器后可通过原生接口解封。", "Start the server to unban through its native interface.")
        : props.state.loading ? selectLocaleText(props.locale, "封禁名单正在读取，请等待完成。", "Wait for the ban roster to finish loading.")
          : props.state.stale || props.state.error || !props.state.roster ? selectLocaleText(props.locale,
            "请先成功刷新封禁名单，再执行解封。", "Refresh the ban roster successfully before unbanning.")
            : !props.entry ? props.hasSelection
              ? selectLocaleText(props.locale, "所选账号不在封禁名单中。", "The selected account is not in the ban roster.")
              : selectLocaleText(props.locale, "请先选择玩家或名单条目。", "Select a player or roster entry first.") : null;
  async function unban() {
    const entry = props.entry;
    const scope = scopeRef.current;
    if (!entry || !scope || reason || inFlightRef.current) return;
    inFlightRef.current = true;
    setBusy(true);
    props.onBusyChange(true);
    setFeedback(null);
    try {
      await executeDeclaredRuntimePlayerAction(props.instanceId, "unban_player", entry.account_id, "");
      if (scopeRef.current !== scope) return;
      try {
        await props.onCompleted();
        if (scopeRef.current === scope) setFeedback({ accountId: entry.account_id, tone: "success",
          text: selectLocaleText(props.locale, `已确认解除封禁：${entry.account_id}`, `Unban confirmed: ${entry.account_id}`) });
      } catch (cause) {
        if (scopeRef.current === scope) setFeedback({ accountId: entry.account_id, tone: "warning",
          text: selectLocaleText(props.locale, `解封已确认，但刷新失败：${describeError(cause)}`,
            `Unban confirmed, but refresh failed: ${describeError(cause)}`) });
      }
    } catch (cause) {
      if (scopeRef.current === scope) {
        props.state.invalidate();
        setFeedback({ accountId: entry.account_id, tone: "error", text: describeError(cause) });
      }
    } finally {
      inFlightRef.current = false;
      if (scopeRef.current === scope) { setBusy(false); props.onBusyChange(false); }
    }
  }
  const visibleFeedback = feedback && (!props.entry || feedback.accountId === props.entry.account_id) ? feedback : null;
  return <section className="player-center-member-pane dragonwilds-ban-actions" aria-busy={busy}
    aria-label={selectLocaleText(props.locale, "封禁名单操作", "Ban roster actions")}>
    <section className="player-center-action-group"><div className="player-center-action-list">
      <InlineConfirmAction className="secondary-button player-center-action-button dragonwilds-unban-action" disabled={Boolean(reason)}
        scopeKey={JSON.stringify([props.instanceId, props.entry?.account_id, props.state.roster?.observed_at_unix_ms])}
        confirmation={selectLocaleText(props.locale, `确认解除 EOS 账号「${props.entry?.account_id ?? ""}」的封禁？`,
          `Unban EOS account “${props.entry?.account_id ?? ""}”?`)} onConfirm={unban}>
        {selectLocaleText(props.locale, "解除封禁", "Unban")}
      </InlineConfirmAction>
      {reason ? <p className="player-center-action-reason" role="status">{reason}</p> : null}
    </div></section>
    {visibleFeedback ? <ActivityNotice tone={visibleFeedback.tone}>{visibleFeedback.text}</ActivityNotice> : null}
  </section>;
}
