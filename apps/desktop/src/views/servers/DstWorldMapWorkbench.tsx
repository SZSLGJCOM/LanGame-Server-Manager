import { useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { DstMapCachedView } from "../../dst-map-cache";
import { dstMapWorldKey } from "../../dst-map-session";
import { readDstMapSelectedShard, rememberDstMapSelectedShard } from "../../dst-map-preferences";
import { mapPointToWorld, type DstMapClick, type DstMapPlayer } from "../../dst-world-map";
import { selectLocaleText, useI18n } from "../../i18n";
import { runtimeProcessKeyIsRunning } from "../../runtime-action-state";
import type { InstanceDetails } from "../../types";
import { DONTSTARVE_SHARDS, DONTSTARVE_SHARD_NAMES, isDontStarveShardActive, type DontStarveShard } from "../settings/modules/dontstarve-shards";
import { DstWorldMapCanvas } from "./DstWorldMapCanvas";
import { DstMapArtworkStatus } from "./DstMapArtworkStatus";
import { DstMapZoomControls } from "./DstMapZoomControls";
import { useDstMapSession } from "./useDstMapSession";
import { useDstMapFollow } from "./useDstMapFollow";
import { DstMapCollectionStatus } from "./DstMapCollectionStatus";
import { DstWorldRegeneration, useDstWorldRegeneration } from "./DstWorldRegeneration";
import { DstMapRoster } from "./DstMapRoster";
import { DstWorldToolsPanel, type DstMapPick } from "./DstWorldToolsPanel";
import { DstItemSpawnPicker, DstItemSpawnTools, useDstItemSpawn } from "./DstItemSpawnPanel";
import { DstGmCommandStatus } from "./DstGmCommandStatus";
import { useDstGmCommand } from "./use-dst-gm-command";
import "./dst-world-map.css";

export const DST_GM_PANELS = ["world", "items"] as const;
export type DstGmPanel = typeof DST_GM_PANELS[number];

interface Props { details: InstanceDetails; panel: DstGmPanel }

/** DST GM surface: the world map is always shown; the rail swaps between world tools and item spawning. */
export function DstWorldMapWorkbench({ details, panel }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const instanceId = details.summary.id, runId = details.active_run?.run_id;
  const enabled = useMemo(() => {
    try { const settings = JSON.parse(details.settings_json); return DONTSTARVE_SHARDS.filter(shard => isDontStarveShardActive(settings, shard)); }
    catch { return ["master"] as DontStarveShard[]; }
  }, [details.settings_json]);
  const rememberedShard = useMemo(() => readDstMapSelectedShard(instanceId), [instanceId]);
  const [selection, setSelection] = useState({ instanceId, shard: rememberedShard });
  const desiredShard = selection.instanceId === instanceId ? selection.shard : rememberedShard;
  const shard = enabled.includes(desiredShard) ? desiredShard : "master";
  const running = runtimeProcessKeyIsRunning(details.active_run, shard);
  // Regeneration state outlives this view: after it only maps from the new world's run are valid.
  const regeneration = useDstWorldRegeneration(instanceId);
  const regenerate = <DstWorldRegeneration key={instanceId} details={details} />;
  const map = useDstMapSession(instanceId, shard, runId, running, regeneration.freshFromRunId);
  const current = map.snapshot;
  const worldKey = current ? dstMapWorldKey(current) : "";
  const [view, setView] = useState<{ key: string; value: DstMapCachedView } | null>(null);
  const currentView = view?.key === worldKey ? view.value : map.initialView;
  const viewRef = useRef({ key: worldKey, value: currentView });
  viewRef.current = { key: worldKey, value: currentView };
  const zoom = currentView?.zoom ?? 1;
  const [zoomMaximum, setZoomMaximum] = useState(10);
  const [fitRevision, setFitRevision] = useState(0);
  const [follow, setFollow] = useState<{ instanceId: string; runId: number | undefined; id: string; name: string } | null>(null);
  const selectedPlayer = follow?.instanceId === instanceId && follow.runId === runId ? follow.id : null;
  const [canvasDetailsTarget, setCanvasDetailsTarget] = useState<HTMLDivElement | null>(null);
  const followingPlayer = current?.players.find(player => player.id === selectedPlayer);
  const command = useDstGmCommand(details);
  const spawn = useDstItemSpawn();
  // The world grid never depends on a loaded map; each button explains why it cannot run.
  const unavailable = regeneration.busy
    ? text("世界正在重新生成，进度见底部动态栏。", "The world is regenerating; see the activity bar.") : null;
  const masterReason = unavailable ?? (runtimeProcessKeyIsRunning(details.active_run, "master") ? null
    : text("主世界未运行，请先启动服务器。", "The Master shard is not running. Start the server first."));
  const playerReason = unavailable ?? (running ? null : text(`分片 ${shard} 未运行。`, `Shard ${shard} is not running.`));
  const [pickState, setPick] = useState<{ target: string; mode: DstMapPick } | null>(null);
  const target = selectedPlayer ? { id: selectedPlayer, name: followingPlayer?.name || follow?.name || selectedPlayer } : null;
  const following = useDstMapFollow({ instanceId, runId, shard,
    runningShards: enabled.filter(value => runtimeProcessKeyIsRunning(details.active_run, value)),
    selectedPlayer, current, live: map.live && running,
    onShard: selectShard });

  const pick = pickState && pickState.target === selectedPlayer && !playerReason ? pickState.mode : null;
  // Teleport destinations always come from the viewed snapshot: the selected player is on this shard.
  const pointReason = followingPlayer ? null : text("所选玩家不在当前地图上", "The selected player is not on this map");
  function teleport(values: Record<string, string>) {
    setPick(null);
    if (target) void command.run(values.destinationId ? "dst_teleport_to_player" : "dst_teleport_to_point", { shard, playerId: target.id, ...values });
  }
  function choose(player: DstMapPlayer) {
    if (pick === "player") { if (player.id !== target?.id) teleport({ destinationId: player.id }); return; }
    toggle(player);
  }
  function mapClick(click: DstMapClick) {
    if (pick === "point" && current && followingPlayer) { const point = mapPointToWorld(current, followingPlayer, click); teleport({ x: String(point.x), z: String(point.z) }); return; }
    const player = current?.players.find(item => item.id === click.playerId);
    if (player) choose(player);
  }
  function selectShard(value: DontStarveShard) {
    setSelection({ instanceId, shard: value });
    rememberDstMapSelectedShard(instanceId, value);
  }
  function remember(next: DstMapCachedView) {
    if (!current) return;
    viewRef.current = { key: worldKey, value: next };
    setView(previous => previous?.key === worldKey && previous.value.zoom === next.zoom
      && previous.value.center.x === next.center.x && previous.value.center.y === next.center.y
      ? previous : { key: worldKey, value: next });
    map.rememberView(current, next);
  }
  function setZoom(next: number) {
    if (current) remember({ zoom: next, center: viewRef.current.value?.center ?? { x: current.width / 2, y: current.height / 2 } });
  }
  function fitMap() {
    if (current) remember({ zoom: 1, center: { x: current.width / 2, y: current.height / 2 } });
    setFollow(null); setFitRevision(value => value + 1);
  }
  const worldLabel = (value: string) => locale === "zh-CN" ? ({ spring: "春季", summer: "夏季", autumn: "秋季", winter: "冬季", day: "白天", dusk: "黄昏", night: "夜晚" } as Record<string, string>)[value] ?? value : value;
  const timestamp = (value: number) => new Date(value).toLocaleString(locale);
  const toggle = (player: DstMapPlayer) =>
    setFollow(selectedPlayer === player.id ? null : { instanceId, runId, id: player.id, name: player.name || player.id });
  // The rail is a column: status and panel content flex above, the player strip and GM grid stay pinned to the bottom.
  function sidebar() {
    const waiting = selectedPlayer && running && !(followingPlayer && map.live);
    return <aside className="dst-map-sidebar" aria-label={text("地图信息", "Map information")}>
      <div className="dst-map-rail-main">
        <div className="dst-map-toolbar">
          <select className="settings-schema-select" aria-label={text("分片", "Shard")} value={shard} onChange={event => {
            selectShard(event.target.value as DontStarveShard); setFollow(null);
          }}>
            {enabled.map(value => <option key={value} value={value}>{DONTSTARVE_SHARD_NAMES[value]}</option>)}
          </select>
          {/* Every rail section stays mounted; without a map they show placeholders, never vanish. */}
          <span className="dst-map-status" title={current ? `${text("地形采集", "Terrain collected")} · ${timestamp(map.terrainAt)}
${text("玩家采集", "Players collected")} · ${timestamp(current.capturedAtUnixMs)}` : undefined}>
            {current ? <>{text(`第 ${current.day} 天`, `Day ${current.day}`)} · {worldLabel(current.season)} · {worldLabel(current.phase)}</> : "—"}
          </span>
        </div>
        <DstMapCollectionStatus error={map.error} />
        {panel === "items" ? <DstItemSpawnPicker spawn={spawn} /> : null}
        <DstGmCommandStatus command={command} />
        <div ref={setCanvasDetailsTarget} className="dst-map-canvas-details" />
        {current ? <>
          {current.landmarksTruncated ? <ActivityNotice tone="warning">{text("地标达到采集上限，部分对象未显示。", "The landmark collection limit was reached; some objects are not shown.")}</ActivityNotice> : null}
          {current.art?.issues.length ? <DstMapArtworkStatus art={current.art} /> : null}
          {map.cacheFailed ? <p role="status" className="dst-map-note">{text("离线副本未能保存；下次打开需重新连接服务器。", "The offline copy could not be saved. Reconnect to the server next time.")}</p> : null}
        </> : null}
      </div>
      <div className="dst-map-rail-dock">
        <DstMapRoster players={current?.players ?? []} art={current?.art ?? null} selectedId={selectedPlayer}
          empty={regeneration.busy ? null
            : !current ? text(running ? "等待地图数据…" : "分片未运行。", running ? "Waiting for map data…" : "This shard is not running.") : null}
          status={[pick === "player" ? text("点击目标玩家完成传送。", "Pick the destination player.") : pick === "point" ? text("在地图上点击目的地。", "Click the destination on the map.") : "",
          waiting ? text(`等待 ${target?.name} 进入分片。`, `Waiting for ${target?.name} on a shard.`) : "",
          following.error ? text("部分分片暂未响应。", "A shard has not responded.") : ""].filter(Boolean).join(" ") || null}
          onClear={() => setFollow(null)} onToggle={choose} />
        {panel === "items" ? <DstItemSpawnTools spawn={spawn} command={command} shard={shard} target={target} /> : null}
        {panel === "world" ? <DstWorldToolsPanel command={command} season={current?.season ?? null} phase={current?.phase ?? null} shard={shard}
          target={target} reasons={{ world: masterReason, player: playerReason, point: pointReason }} regenerate={regenerate}
          pick={pick} onPick={mode => setPick(mode && selectedPlayer ? { target: selectedPlayer, mode } : null)} /> : null}
      </div>
    </aside>;
  }
  return <section className="dst-world-map" aria-label={text("世界地图", "World map")}>
    <div className="dst-map-layout">
      {current ? <DstWorldMapCanvas key={`canvas:${worldKey}`} snapshot={current} selectedPlayer={selectedPlayer} zoom={zoom}
        initialCenter={currentView?.center} onCenterChange={center => remember({ center, zoom: viewRef.current.value?.zoom ?? zoom })}
        fitRevision={fitRevision} onZoomChange={setZoom} onFit={fitMap}
        onCancelFollow={() => setFollow(null)} onZoomLimitChange={setZoomMaximum}
        onMapClick={mapClick}
        controls={<DstMapZoomControls zoom={zoom} maximum={zoomMaximum} onChange={value => { setFollow(null); setZoom(value); }} onFit={fitMap} />}
        // Only canvas-owned details move with the map. Instance operations must
        // survive stopping, cache loading and selecting a different shard.
        renderDetails={content => canvasDetailsTarget ? createPortal(content, canvasDetailsTarget) : null} /> :
        <div className="dst-map-empty" role="status">{regeneration.busy ? text("正在生成新世界…", "Generating the new world…") : !running
          ? text("启动分片后显示世界地图", "Start the shard to see its world map")
          : map.loading ? text("正在读取世界地图…", "Reading the world map…")
          : text("等待服务器返回地图", "Waiting for the server map")}</div>}
      {sidebar()}
    </div>
  </section>;
}
