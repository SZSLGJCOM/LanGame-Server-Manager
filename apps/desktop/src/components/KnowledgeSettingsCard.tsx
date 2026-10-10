import { useState } from "react";
import { useI18n, isChineseLocale } from "../i18n";
import { useKnowledgeSync } from "../hooks/useKnowledgeSync";
import { isKnowledgeJobActive, type KnowledgeApi } from "../knowledge-types";
import type { ModuleSummary } from "../types";
import "./knowledge-settings.css";

interface KnowledgeSettingsCardProps {
  api?: KnowledgeApi;
  modules?: ReadonlyArray<Pick<ModuleSummary, "id" | "name">>;
  preferredModuleId?: string | null;
}

export function KnowledgeSettingsCard({ api, modules = [], preferredModuleId }: KnowledgeSettingsCardProps) {
  const { locale } = useI18n();
  const chinese = isChineseLocale(locale);
  const state = useKnowledgeSync(api);
  const [chosenModuleId, setChosenModuleId] = useState<string | null>(null);
  const copy = chinese ? {
    title: "LAN 开服知识库", description: "优先收集官方开服正文，社区资料单独标注，供 LAN 查阅并引用来源。",
    localOnly: "请在 LGSM 桌面主机上管理知识库更新。", loading: "正在读取知识库…", reload: "重新读取状态",
    auto: "自动更新", interval: "更新间隔", hours: "小时", update: "更新此游戏", updateAll: "更新全部游戏", game: "游戏资料", allGames: "全部游戏", cancel: "取消更新", cancelling: "正在取消并保留已发布索引…",
    modelReady: "本地检索模型已就绪", modelPending: "首次更新将下载本地模型", local: "文档缓存与语义检索在管理端本机完成，不调用付费嵌入 API；AI 回答时，选中的文档片段会随上下文发送给配置的模型服务。",
    games: "个游戏资料目录", cached: "已缓存", docs: "篇正文", chunks: "个索引片段", sources: "来源与覆盖范围", none: "尚未收录正文，请先更新知识库。",
    retained: "先前收录的正文与索引已保留。", checked: "最近检查", success: "最近成功", neverChecked: "尚未检查", never: "尚未成功同步",
    error: "知识库操作未完成，请查看更新详情。", schedulerError: "自动更新异常，请查看更新详情。", save: "正在保存…", noSource: "暂无正文来源", waiting: "正在准备更新…",
    updateDetails: "更新详情", libraryDetails: "索引与数据说明", notUpdated: "暂无更新任务记录。", finished: "完成时间", noUpdateErrors: "本次更新没有报告错误。",
    sourceStates: { ready: "已收录", pending: "尚未收录", error: "更新失败", restricted: "来源受限", interrupted: "更新中断", unknown: "状态未知" },
    cacheNote: "缓存数量包含保留的旧正文；受限来源不会提供给 LAN 检索。资料目录覆盖不代表每个游戏都有完整正文。",
    download: "下载本地语义模型", modelLoading: "加载本地语义模型", documents: "更新正文与索引",
    official: "官方发布", officialCommunity: "官方社区", community: "社区维护",
    states: { running: "正在更新", cancelling: "正在取消", completed: "更新完成", partial: "部分来源未更新", failed: "更新失败", cancelled: "已取消更新", interrupted: "上次更新中断" }
  } : {
    title: "LAN server knowledge", description: "Prioritize official server documentation, label community references separately, and let LAN read and cite sources.",
    localOnly: "Manage knowledge updates on the LGSM desktop host.", loading: "Loading knowledge status…", reload: "Reload status",
    auto: "Automatic updates", interval: "Update interval", hours: "hours", update: "Update this game", updateAll: "Update all games", game: "Game documentation", allGames: "All games", cancel: "Cancel update", cancelling: "Cancelling; keeping the published index…",
    modelReady: "Local retrieval model is ready", modelPending: "The first update downloads the local model", local: "Document caching and semantic retrieval run on the management host without paid embedding APIs. When AI answers, selected excerpts are sent with context to the configured model service.",
    games: "game documentation catalogs", cached: "Cached", docs: "documents", chunks: "indexed chunks", sources: "Sources and coverage", none: "No documents yet. Update the knowledge library first.",
    retained: "Previously collected documents and indexes are retained.", checked: "Last checked", success: "Last success", neverChecked: "Not checked yet", never: "Not synchronized yet",
    error: "The knowledge operation did not finish. Review update details.", schedulerError: "Automatic updates encountered an error. Review update details.", save: "Saving…", noSource: "No document sources", waiting: "Preparing update…",
    updateDetails: "Update details", libraryDetails: "Index and data details", notUpdated: "No update task record is available.", finished: "Finished", noUpdateErrors: "No errors were reported for this update.",
    sourceStates: { ready: "Collected", pending: "Not collected", error: "Update failed", restricted: "Source restricted", interrupted: "Update interrupted", unknown: "Unknown state" },
    cacheNote: "Cached counts include retained documents. Restricted sources are unavailable to LAN retrieval. Catalog coverage does not mean every game has complete documentation.",
    download: "Downloading local semantic model", modelLoading: "Loading local semantic model", documents: "Updating documents and index",
    official: "Official publication", officialCommunity: "Official community", community: "Community maintained",
    states: { running: "Updating", cancelling: "Cancelling", completed: "Updated", partial: "Some sources were not updated", failed: "Update failed", cancelled: "Update cancelled", interrupted: "Previous update interrupted" }
  };
  const status = state.status;
  const library = status?.library;
  const job = status?.job;
  const sources = library?.games.flatMap((game) => game.sources) ?? [];
  const selectedGame = library?.games.find((game) => game.moduleId === chosenModuleId)
    ?? library?.games.find((game) => game.moduleId === preferredModuleId)
    ?? library?.games.find((game) => game.sources.length > 0)
    ?? library?.games[0];
  const gameName = (moduleId: string) => modules.find((module) => module.id === moduleId)?.name ?? moduleId;
  const documents = sources.reduce((total, source) => total + source.documentCount, 0);
  const chunks = sources.reduce((total, source) => total + source.chunkCount, 0);
  const active = isKnowledgeJobActive(job);
  const busy = state.pending !== null;
  const warning = job && ["partial", "failed", "cancelled", "interrupted"].includes(job.state);
  const errors = [...new Set([state.error, status?.schedulerError, job?.error, ...(job?.report?.errors ?? [])].filter((error): error is string => Boolean(error)))];
  const time = (value: number | null) => value ? new Date(value * 1000).toLocaleString(locale) : copy.never;
  const progress = job?.progress;
  const bytes = progress?.totalDownloadBytes ? `${Math.round(progress.downloadedBytes / 1_000_000)} / ${Math.round(progress.totalDownloadBytes / 1_000_000)} MB` : null;
  const progressText = bytes ?? (progress && progress.total > 0 ? `${progress.completed} / ${progress.total}` : copy.waiting);
  const phase = progress?.phase === "model_download" ? copy.download : progress?.phase === "model_loading" ? copy.modelLoading : progress?.phase === "documents" ? copy.documents : null;
  const sourceState = (value: string) => {
    if (value === "ready") return copy.sourceStates.ready;
    if (value === "error") return copy.sourceStates.error;
    if (value === "restricted") return copy.sourceStates.restricted;
    if (value === "interrupted") return copy.sourceStates.interrupted;
    return value === "pending" ? copy.sourceStates.pending : copy.sourceStates.unknown;
  };

  return <section className="knowledge-settings" aria-labelledby="knowledge-settings-title">
    <div className="knowledge-settings-heading"><h3 id="knowledge-settings-title">{copy.title}</h3></div>
    {!state.available ? <p>{copy.localOnly}</p> : <>
      {!library && state.loading ? <p role="status">{copy.loading}</p> : null}
      {state.error ? <p className="knowledge-settings-error" role="alert">{copy.error}</p>
        : status?.schedulerError ? <p className="knowledge-settings-warning" role="status">{copy.schedulerError}</p> : null}
      {library ? <>
        <div className="knowledge-settings-library-state">
          <strong className="knowledge-settings-counts">{copy.cached} {documents} {copy.docs}</strong>
          <span className={`knowledge-settings-model-state${library.model.ready ? " is-ready" : ""}`}>{library.model.ready ? copy.modelReady : `${copy.modelPending}（${Math.ceil(library.model.downloadBytes / 1_000_000)} MB）`}</span>
        </div>
        {!documents ? <p>{copy.none}</p> : null}
        <div className="knowledge-settings-controls">
          <label className="knowledge-settings-auto"><input type="checkbox" checked={library.settings.autoUpdate} disabled={busy}
            onChange={(event) => { void state.save({ ...library.settings, autoUpdate: event.target.checked }); }} />{copy.auto}</label>
          <label className="knowledge-settings-interval">{copy.interval}<select aria-label={copy.interval} value={library.settings.intervalHours} disabled={busy}
            onChange={(event) => { void state.save({ ...library.settings, intervalHours: Number(event.target.value) }); }}>
            {[...new Set([6, 12, 24, 48, 72, 168, library.settings.intervalHours])].sort((a, b) => a - b).map((hours) => <option key={hours} value={hours}>{hours} {copy.hours}</option>)}
          </select></label>
        </div>
        {selectedGame ? <label className="knowledge-settings-game">{copy.game}<select aria-label={copy.game} value={selectedGame.moduleId}
          onChange={(event) => { setChosenModuleId(event.target.value); }}>
          {library.games.map((game) => <option key={game.moduleId} value={game.moduleId}>{gameName(game.moduleId)}</option>)}
        </select></label> : null}
        <div className="knowledge-settings-actions">
          <button className="secondary-button" type="button" disabled={busy || active || !selectedGame?.sources.length} onClick={() => { if (selectedGame) void state.start(selectedGame.moduleId); }}>{copy.update}</button>
          <button className="secondary-button" type="button" disabled={busy || active || !sources.length} onClick={() => { void state.start(null); }}>{copy.updateAll}</button>
          {active ? <button className="secondary-button" type="button" disabled={busy || job?.state === "cancelling"} onClick={() => { void state.cancel(); }}>{copy.cancel}</button> : null}
          {state.pending === "save" ? <span role="status">{copy.save}</span> : null}
        </div>
        {job ? <div role="status" className={`knowledge-settings-progress${warning ? " is-warning" : ""}`}>
          <p><strong>{copy.states[job.state]}</strong> · {job.moduleId ? gameName(job.moduleId) : copy.allGames}{warning && documents > 0 ? ` · ${copy.retained}` : ""}</p>
          {active ? <>{phase ? <span>{phase}</span> : null}<span>{job.state === "cancelling" ? copy.cancelling : progressText}</span>{progress?.moduleId && progress.moduleId !== job.moduleId ? <span>{gameName(progress.moduleId)}</span> : null}
            {progress && progress.total > 0 ? <progress max={progress.total} value={Math.min(progress.completed, progress.total)} aria-label={copy.states.running} /> : null}</> : null}
        </div> : null}
      </> : null}
      <details className="knowledge-settings-update-details">
        <summary>{copy.updateDetails}</summary>
        <div className="knowledge-settings-detail-body">
          {job ? <><p>{copy.states[job.state]} · {job.moduleId ? gameName(job.moduleId) : copy.allGames}</p>
            {job.finishedAt ? <p className="knowledge-settings-muted">{copy.finished}: {time(job.finishedAt)}</p> : null}</> : library ? <p>{copy.notUpdated}</p> : null}
          {errors.length ? <div className="knowledge-settings-diagnostics">{errors.map((error) => <p key={error}>{error}</p>)}</div>
            : job && !active ? <p className="knowledge-settings-muted">{copy.noUpdateErrors}</p> : null}
          <button type="button" className="secondary-button" disabled={state.loading || busy} onClick={() => { void state.reload(); }}>{copy.reload}</button>
        </div>
      </details>
      {library ? <>
        <details className="knowledge-settings-sources"><summary>{copy.sources}</summary>
          {selectedGame ? <section key={selectedGame.moduleId} aria-label={gameName(selectedGame.moduleId)}><h4>{gameName(selectedGame.moduleId)}</h4>
            <p className="knowledge-settings-muted">{selectedGame.scope}</p>
            {!selectedGame.sources.length ? <p>{copy.noSource}</p> : null}
            {selectedGame.gaps.map((gap) => <p className="knowledge-settings-muted" key={gap}>{gap}</p>)}
            {selectedGame.sources.map((source) => <div className="knowledge-settings-source" key={source.id}>
              <div className="knowledge-settings-source-heading"><strong>{source.title}</strong><span className={`knowledge-settings-source-state${source.state === "error" ? " is-warning" : ""}`}>{sourceState(source.state)}</span></div>
              <span className="knowledge-settings-muted">{source.kind === "official" ? copy.official : source.kind === "official_community" ? copy.officialCommunity : copy.community} · {source.authority}</span>
              <a className="knowledge-settings-url" href={source.url} target="_blank" rel="noopener noreferrer">{source.url}</a>
              <span>{copy.cached} {source.documentCount} {copy.docs} · {source.chunkCount} {copy.chunks}</span>
              <span className="knowledge-settings-muted">{copy.checked}: {source.lastCheckedAt ? time(source.lastCheckedAt) : copy.neverChecked}</span><span className="knowledge-settings-muted">{copy.success}: {time(source.lastSuccessAt)}</span>
              {source.lastError ? <p className={source.state === "restricted" ? "knowledge-settings-muted" : "knowledge-settings-source-error"}>{source.lastError}</p> : null}
            </div>)}
          </section> : null}
        </details>
        <details className="knowledge-settings-library-details"><summary>{copy.libraryDetails}</summary>
          <div className="knowledge-settings-detail-body">
            <p className="knowledge-settings-index-counts">{library.games.length} {copy.games} · {chunks} {copy.chunks}</p>
            <p>{copy.description}</p><p className="knowledge-settings-muted">{copy.local}</p><p className="knowledge-settings-muted">{copy.cacheNote}</p>
          </div>
        </details>
      </> : null}
    </>}
  </section>;
}
