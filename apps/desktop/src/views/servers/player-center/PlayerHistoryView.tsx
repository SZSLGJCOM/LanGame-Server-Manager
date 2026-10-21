import { useEffect, useId, useRef, useState } from "react";
import type { LocaleCode } from "../../../i18n";
import { readInstancePlayerHistory, type PlayerHistoryKind, type PlayerHistoryPage, type PlayerHistoryQuery } from "../../../player-history";
import "./player-history.css";

interface Props { instanceId: string; locale: LocaleCode }
const initialFilters = { kind: "", search: "", since: "", until: "" };

export function PlayerHistoryView(props: Props) {
  return <PlayerHistoryInstance key={props.instanceId} {...props} />;
}

function PlayerHistoryInstance({ instanceId, locale }: Props) {
  const text = (zh: string, en: string) => locale === "zh-CN" ? zh : en;
  const id = useId();
  const [filters, setFilters] = useState(initialFilters);
  const [query, setQuery] = useState<PlayerHistoryQuery>({ instance_id: instanceId, limit: 50 });
  const [page, setPage] = useState<PlayerHistoryPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const request = useRef(0);
  const [previous, setPrevious] = useState<(number | undefined)[]>([]);
  useEffect(() => {
    const current = ++request.current;
    let active = true;
    setLoading(true); setError(null); setPage(null);
    void readInstancePlayerHistory(query).then((result) => {
      if (active && current === request.current && result.instance_id === instanceId) setPage(result);
    }).catch((cause: unknown) => {
      if (active && current === request.current) setError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => { if (active && current === request.current) setLoading(false); });
    return () => { active = false; };
  }, [query, refresh, instanceId]);
  const labels: Record<PlayerHistoryKind, string> = {
    join: text("进入", "Joined"), leave: text("离开", "Left"), chat: text("聊天", "Chat")
  };
  const duration = (milliseconds: number | null) => {
    if (milliseconds === null) return text("未确认", "Unconfirmed");
    const seconds = Math.floor(milliseconds / 1000);
    return `${Math.floor(seconds / 3600)}:${String(Math.floor(seconds / 60) % 60).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
  };
  function applyFilters() {
    const since = filters.since ? new Date(filters.since).getTime() : undefined;
    const until = filters.until ? new Date(filters.until).getTime() : undefined;
    if (since !== undefined && until !== undefined && since > until) {
      setError(text("开始时间不能晚于结束时间。", "The start must be before the end.")); return;
    }
    setPrevious([]);
    setQuery({ instance_id: instanceId, limit: 50, search: filters.search.trim() || undefined,
      kind: filters.kind ? filters.kind as PlayerHistoryKind : undefined, since_unix_ms: since, until_unix_ms: until });
  }
  return <section className="player-history" aria-label={text("玩家历史", "Player history")} aria-busy={loading}>
    <form className="player-history-filters" onSubmit={(event) => { event.preventDefault(); applyFilters(); }}>
      <label htmlFor={`${id}-search`}>{text("玩家 / 账号 / 聊天", "Player / account / chat")}
        <input id={`${id}-search`} className="settings-schema-input" maxLength={256} value={filters.search}
          onChange={(event) => setFilters({ ...filters, search: event.target.value })} /></label>
      <label htmlFor={`${id}-kind`}>{text("记录类型", "Event type")}
        <select id={`${id}-kind`} className="settings-schema-select" value={filters.kind}
          onChange={(event) => setFilters({ ...filters, kind: event.target.value })}>
          <option value="">{text("全部", "All")}</option>{Object.entries(labels).map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}
        </select></label>
      <label htmlFor={`${id}-since`}>{text("开始时间", "From")}
        <input id={`${id}-since`} className="settings-schema-input" type="datetime-local" value={filters.since}
          onChange={(event) => setFilters({ ...filters, since: event.target.value })} /></label>
      <label htmlFor={`${id}-until`}>{text("结束时间", "Until")}
        <input id={`${id}-until`} className="settings-schema-input" type="datetime-local" value={filters.until}
          onChange={(event) => setFilters({ ...filters, until: event.target.value })} /></label>
      <button type="submit" className="secondary-button" disabled={loading}>{text("查询", "Search")}</button>
      <button type="button" className="secondary-button" disabled={loading} onClick={() => { setPrevious([]); setQuery({ ...query, before_id: undefined }); setRefresh((value) => value + 1); }}>{text("刷新", "Refresh")}</button>
    </form>
    <p className="player-history-note">{text("记录来自各分片原生日志。日期按进程启动时间推算；在线时长仅显示同次运行中可配对的进出记录。同名或缺失记录不合并，未确认离开不代表仍在线。", "Records come from shard logs. Dates are estimated from process start; duration requires matching join and leave records in the same run. Ambiguous or missing records are not merged. An unconfirmed departure does not mean the player is online.")}</p>
    {error ? <p role="alert" className="form-note form-note--error">{error}</p> : null}
    {page?.collection_issue ? <p role="status" className="form-note form-note--error">{text("部分日志不可用、已被清理或时间不连续，历史可能不完整。", "Some logs are unavailable, retained logs have gaps, or timestamps are discontinuous; history may be incomplete.")}</p> : null}
    <div className="player-history-results" role="status" aria-live="polite">
      {loading ? <p className="player-history-empty">{text("正在读取历史…", "Loading history…")}</p>
        : page && !page.supported ? <p className="player-history-empty">{text("此游戏尚无原生历史采集器。", "This game has no native history collector.")}</p>
        : page?.events.length === 0 ? <p className="player-history-empty">{text("没有符合条件的记录。已保存的服务器日志会在后台采集。", "No matching records. Retained server logs are collected in the background.")}</p>
        : page ? <table className="player-history-table"><thead><tr>
          <th>{text("时间 / 分片", "Time / shard")}</th><th>{text("玩家", "Player")}</th><th>{text("记录", "Event")}</th><th>{text("内容 / 在线时长", "Message / duration")}</th>
        </tr></thead><tbody>{page.events.map((event) => <tr key={event.id}>
          <td><span>{event.estimated_at_unix_ms === null ? text("日期未知", "Unknown date") : new Date(event.estimated_at_unix_ms).toLocaleString(locale)}</span><small>{event.process_key} · {event.log_time} · #{event.run_id}</small></td>
          <td><span>{event.player_name}</span>{event.account_id ? <small>{event.account_id}</small> : null}</td>
          <td>{labels[event.kind]}</td><td>{event.kind === "chat" ? event.message : duration(event.duration_ms)}</td>
        </tr>)}</tbody></table> : null}
    </div>
    <div className="player-history-pagination">
      <button type="button" className="secondary-button" disabled={loading || previous.length === 0} onClick={() => {
        const cursor = previous[previous.length - 1]; setPrevious(previous.slice(0, -1)); setQuery({ ...query, before_id: cursor });
      }}>{text("上一页", "Previous")}</button>
      <span>{text(`第 ${previous.length + 1} 页`, `Page ${previous.length + 1}`)}</span>
      <button type="button" className="secondary-button" disabled={loading || !page?.next_before_id} onClick={() => {
        if (!page?.next_before_id) return;
        setPrevious([...previous, query.before_id]); setQuery({ ...query, before_id: page.next_before_id });
      }}>{text("下一页", "Next")}</button>
    </div>
  </section>;
}
