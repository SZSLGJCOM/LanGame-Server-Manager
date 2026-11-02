import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { sendArkMapGmCommand, type ArkToolsStatus } from "../../api-ark-tools";
import { arkMapScopeKey, arkMapTargets, type ArkMapPlayer, type ArkMapScope } from "../../ark-world-map";
import { ActivityNotice } from "../../components/ActivityNotice";
import { selectLocaleText, useI18n } from "../../i18n";
import { runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import type { InstanceDetails, InstanceRuntimeCommandResult } from "../../types";
import { ArkWorldMapCanvas } from "./ArkWorldMapCanvas";
import { useArkMapSession } from "./useArkMapSession";
import { ARK_FULL_GPS_BOUNDS } from "../../ark-map-artwork-catalog";
import { useArkMapArtwork } from "./useArkMapArtwork";
import "./ark-world-map.css";

export interface ArkMapToolContext {
  scope: ArkMapScope | null; scopeKey: string; running: boolean; live: boolean; selectedPlayer: ArkMapPlayer | null;
  extension: ArkToolsStatus | null;
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
  const automaticArtwork = useArkMapArtwork(details.summary.module_id, mapName, visible);
  const snapshot = map.snapshot;
  const localArtwork = snapshot?.artwork;
  const artwork = localArtwork ? { url: localArtwork.dataUrl, source: localArtwork.source, bounds: ARK_FULL_GPS_BOUNDS } : automaticArtwork.artwork;
  const scopeKey = `${scope ? arkMapScopeKey(scope) : JSON.stringify([instanceId, processKey])}:${snapshot?.mapName ?? ""}:${running}`;
  const currentScope = useRef(scopeKey); currentScope.current = scopeKey;
  const [chosen, setChosen] = useState<{ key: string; playerId: number } | null>(null);
  const selectedId = chosen?.key === scopeKey ? chosen.playerId : null;
  const selected = snapshot?.players.find(player => player.playerId === selectedId) ?? null;
  const [following, setFollowing] = useState(false);
  const livePlayer = map.live ? selected : null;
  const canRefreshPlayers = Boolean(scope && running && map.host);
  const context: ArkMapToolContext = { scope, scopeKey, running, live: map.live, selectedPlayer: livePlayer, extension: map.extension,
    async send(id, command) {
      if (!scope || !running || id !== instanceId || currentScope.current !== scopeKey) throw new Error(text("地图或运行批次已改变，剩余命令已停止。", "The map or runtime changed. Remaining commands were stopped."));
      return sendArkMapGmCommand(scope, command);
    } };
  function choose(player: ArkMapPlayer) {
    if (!map.live) return;
    const selecting = selectedId !== player.playerId;
    setChosen(selecting ? { key: scopeKey, playerId: player.playerId } : null);
    setFollowing(selecting);
  }
  return <section ref={panel} className="ark-world-map" aria-label={text("方舟地图工作台", "ARK map workbench")}>
    <div className="ark-map-layout">
    <ArkWorldMapCanvas key={`${instanceId}:${processKey}:${runId ?? 0}:${snapshot?.mapName ?? ""}`} snapshot={snapshot} live={map.live}
      artwork={artwork} artworkLoading={automaticArtwork.loading && !localArtwork}
      selectedId={selectedId} follow={following} onSelect={choose} onStopFollow={() => setFollowing(false)} />
    <aside className="ark-map-rail" aria-label={text("地图工具与玩家", "Map tools and players")}>
      <div className="ark-map-toolbar"><select className="settings-schema-select" aria-label={text("地图", "Map")} value={processKey}
        onChange={event => { setSelection({ instanceId, processKey: event.target.value }); setChosen(null); setFollowing(false); }}>
        {targets.map(target => <option key={target.processKey} value={target.processKey}>{target.label}</option>)}
      </select><button className="secondary-button" type="button" disabled={!canRefreshPlayers && !automaticArtwork.canRetry}
        onClick={() => { if (canRefreshPlayers) map.refresh(); automaticArtwork.refresh(); }}>{text("刷新", "Refresh")}</button></div>
      {map.error || map.extension?.issue ? <ActivityNotice tone="warning">{map.error || map.extension?.issue || ""}</ActivityNotice> : null}
      <section className="ark-map-roster" aria-label={text("在线玩家", "Online players")} aria-busy={map.loading}>
        <div className="ark-map-roster-heading"><strong>{text("在线玩家", "Online players")} · {snapshot?.players.length ?? 0}</strong>
          <span>{snapshot ? new Date(snapshot.sampledAtUnixMs).toLocaleTimeString(locale) : "—"}</span></div>
        <div className="ark-map-roster-list">{snapshot?.players.map(player => <button type="button" key={player.playerId}
          disabled={!map.live} aria-pressed={selectedId === player.playerId} onClick={() => choose(player)}
          title={`${player.name || player.playerId} · PlayerID ${player.playerId}${player.x === null ? ` · ${text("无角色坐标", "no character coordinates")}` : ""}`}>
          <strong>{player.name || `Player ${player.playerId}`}</strong>
        </button>)}</div>
      </section>
      <div className="ark-map-tools">{children(context)}</div>
    </aside>
    </div>
  </section>;
}
