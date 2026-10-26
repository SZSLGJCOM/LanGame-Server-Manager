import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { sendArkMapGmCommand } from "../../api-ark-tools";
import { arkMapScopeKey, arkMapTargets, type ArkMapPlayer, type ArkMapScope } from "../../ark-world-map";
import { ActivityNotice } from "../../components/ActivityNotice";
import { selectLocaleText, useI18n } from "../../i18n";
import { runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import type { InstanceDetails, InstanceRuntimeCommandResult } from "../../types";
import { ArkWorldMapCanvas } from "./ArkWorldMapCanvas";
import { useArkMapSession } from "./useArkMapSession";
import { ArkMapArtworkControl } from "./ArkMapArtworkControl";
import type { ArkImportedArtwork } from "../../ark-map-artwork";
import { ARK_FULL_GPS_BOUNDS } from "../../ark-map-artwork-catalog";
import { useArkMapArtwork } from "./useArkMapArtwork";
import "./ark-world-map.css";

export interface ArkMapToolContext {
  scope: ArkMapScope | null; scopeKey: string; running: boolean; live: boolean; selectedPlayer: ArkMapPlayer | null;
  send: (instanceId: string, command: string) => Promise<InstanceRuntimeCommandResult>;
}
interface Props { details: InstanceDetails; children: (context: ArkMapToolContext) => ReactNode }
export function ArkWorldMapWorkbench({ details, children }: Props) {
  const panel = useRef<HTMLElement>(null);
  const [visible, setVisible] = useState(true);
  useEffect(() => {
    const node = panel.current; if (!node) return;
    const observer = new IntersectionObserver(entries => setVisible(entries[0]?.isIntersecting ?? false));
    observer.observe(node); return () => observer.disconnect();
  }, []);
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const instanceId = details.summary.id;
  const targets = useMemo(() => arkMapTargets(details.settings_json), [details.settings_json]);
  const [selection, setSelection] = useState({ instanceId, processKey: "main" });
  const processKey = selection.instanceId === instanceId && targets.some(target => target.processKey === selection.processKey) ? selection.processKey : "main";
  const process = details.active_run?.processes?.find(value => value.process_key === processKey);
  const runId = process?.run_id ?? (processKey === "main" && !details.active_run?.processes?.length ? details.active_run?.run_id : null);
  const running = runtimeProcessKeyIsRunning(details.active_run, processKey)
    || processKey === "main" && !details.active_run?.processes?.length && details.summary.status.toLowerCase() === "running";
  const scope = runId && runId > 0 ? { instanceId, processKey, runId } : null;
  const targetMap = targets.find(target => target.processKey === processKey)?.mapName ?? "";
  const map = useArkMapSession(scope, running && visible, targetMap);
  const mapName = map.snapshot?.mapName ?? targetMap;
  const artworkKey = `${details.summary.module_id}:${mapName}`;
  const automaticArtwork = useArkMapArtwork(details.summary.module_id, mapName, visible);
  const [imported, setImported] = useState<{ key: string; value: ArkImportedArtwork | null } | null>(null);
  const snapshot = map.snapshot;
  const localArtwork = snapshot?.artwork ?? (imported?.key === artworkKey ? imported.value : null);
  const artwork = localArtwork ? { url: localArtwork.dataUrl, source: localArtwork.source, bounds: ARK_FULL_GPS_BOUNDS } : automaticArtwork.artwork;
  const scopeKey = `${scope ? arkMapScopeKey(scope) : JSON.stringify([instanceId, processKey])}:${snapshot?.mapName ?? ""}:${running}`;
  const currentScope = useRef(scopeKey); currentScope.current = scopeKey;
  const [chosen, setChosen] = useState<{ key: string; playerId: number } | null>(null);
  const selectedId = chosen?.key === scopeKey ? chosen.playerId : null;
  const selected = snapshot?.players.find(player => player.playerId === selectedId) ?? null;
  const [following, setFollowing] = useState(false);
  const livePlayer = map.live ? selected : null;
  const canRefreshPlayers = Boolean(scope && running && map.host);
  const context: ArkMapToolContext = { scope, scopeKey, running, live: map.live, selectedPlayer: livePlayer,
    async send(id, command) {
      if (!scope || !running || id !== instanceId || currentScope.current !== scopeKey) throw new Error(text("地图或运行批次已改变，剩余命令已停止。", "The map or runtime changed. Remaining commands were stopped."));
      return sendArkMapGmCommand(scope, command);
    } };
  const status = !map.host ? text("请在本机桌面客户端读取实时地图。", "Open the local desktop client to read the live map.")
    : !running || !scope ? text("地图未运行，启动后读取玩家位置。", "This map is stopped. Start it to read player positions.")
    : map.loading ? text("正在读取地图与玩家…", "Reading map and players…")
    : map.error ? text("地图读取失败，已暂停玩家选取。", "Map read failed. Player selection is paused.")
    : !map.extension?.installed ? text("服务端扩展未安装，可在生成生物工具中准备。", "Server extension is not installed. Prepare it in Spawn creature.")
    : !map.extension?.connected ? text("等待服务端扩展连接…", "Waiting for the server extension…")
    : !map.live ? text("玩家位置已过期，等待刷新。", "Player positions are stale. Waiting for a refresh.")
    : snapshot?.players.length === 0 ? text("当前地图没有在线玩家。", "No players are online on this map.")
    : text(`实时玩家 ${snapshot?.players.length ?? 0} · 每 3 秒刷新`, `${snapshot?.players.length ?? 0} live players · refreshes every 3 seconds`);
  function choose(player: ArkMapPlayer) { if (map.live) { setChosen({ key: scopeKey, playerId: player.playerId }); setFollowing(true); } }
  return <section ref={panel} className="ark-world-map" aria-label={text("方舟地图工作台", "ARK map workbench")}>
    <ArkWorldMapCanvas key={`${instanceId}:${processKey}:${runId ?? 0}:${snapshot?.mapName ?? ""}`} snapshot={snapshot} live={map.live}
      mapName={mapName} artwork={artwork} artworkLoading={automaticArtwork.loading && !localArtwork}
      selectedId={selectedId} follow={following} onSelect={choose} onStopFollow={() => setFollowing(false)} status={status} />
    <aside className="ark-map-rail" aria-label={text("地图工具与玩家", "Map tools and players")}>
      <div className="ark-map-toolbar"><label><span>{text("地图", "Map")}</span><select className="gmt-select" aria-label={text("地图", "Map")} value={processKey}
        onChange={event => { setSelection({ instanceId, processKey: event.target.value }); setChosen(null); setFollowing(false); }}>
        {targets.map(target => <option key={target.processKey} value={target.processKey}>{target.label}</option>)}
      </select></label><button className="secondary-button" type="button" disabled={!canRefreshPlayers && !automaticArtwork.canRetry}
        onClick={() => { if (canRefreshPlayers) map.refresh(); automaticArtwork.refresh(); }}>{text("刷新", "Refresh")}</button></div>
      {map.error || map.extension?.issue ? <ActivityNotice tone="warning">{map.error || map.extension?.issue || ""}</ActivityNotice> : null}
      <section className="ark-map-roster" aria-label={text("在线玩家", "Online players")}>
        <div className="ark-map-roster-heading"><strong>{text("在线玩家", "Online players")} · {snapshot?.players.length ?? 0}</strong>
          <span>{snapshot ? new Date(snapshot.sampledAtUnixMs).toLocaleTimeString(locale) : "—"}</span></div>
        <div className="ark-map-roster-list">{snapshot?.players.length ? snapshot.players.map(player => <button type="button" key={player.playerId}
          disabled={!map.live} aria-pressed={selectedId === player.playerId} onClick={() => choose(player)}
          title={`${player.name || player.playerId} · PlayerID ${player.playerId}${player.x === null ? ` · ${text("无角色坐标", "no character coordinates")}` : ""}`}>
          <strong>{player.name || `Player ${player.playerId}`}</strong><span>PlayerID {player.playerId}{player.x === null ? ` · ${text("无角色坐标", "no character coordinates")}` : ""}</span>
        </button>) : <p>{status}</p>}</div>
        {selected ? <div className="ark-map-selected"><span title={selected.accountId || undefined}>{selected.name || selected.playerId} · PlayerID {selected.playerId}</span>
          <small>{selected.accountId ? `${text("平台账号", "Platform account")} ${selected.accountId}` : text("无平台账号", "No platform account")}</small>
          <div><button type="button" className="secondary-button" disabled={!livePlayer || livePlayer.x === null} aria-pressed={following}
            onClick={() => setFollowing(value => !value)}>{following ? text("停止跟随", "Stop following") : text("跟随玩家", "Follow player")}</button>
            <button type="button" className="secondary-button" onClick={() => { setChosen(null); setFollowing(false); }}>{text("取消选择", "Clear selection")}</button></div></div> : null}
      </section>
      <ArkMapArtworkControl key={artworkKey} edition={details.summary.module_id} mapName={map.snapshot?.mapName ?? targetMap}
        calibrated={Boolean(map.snapshot?.projection)} onChange={value => setImported({ key: artworkKey, value })} />
      <div className="ark-map-tools">{children(context)}</div>
    </aside>
  </section>;
}
