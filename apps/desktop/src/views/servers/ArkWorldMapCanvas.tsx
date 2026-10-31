import { useEffect, useMemo, useRef, useState } from "react";
import { arkPlayerPoint, type ArkMapPlayer, type ArkMapSnapshot } from "../../ark-world-map";
import { clampArkMapView } from "../../ark-map-view";
import { selectLocaleText, useI18n } from "../../i18n";
import { arkArtworkRect, type ArkMapArtwork } from "../../ark-map-artwork-catalog";
import { openExternalUrl } from "../../api";

interface Props { snapshot: ArkMapSnapshot | null; live: boolean; selectedId: number | null; follow: boolean;
  mapName: string; artwork: ArkMapArtwork | null; artworkLoading: boolean;
  onSelect: (player: ArkMapPlayer) => void; onStopFollow: () => void; status: string }
export function ArkWorldMapCanvas({ snapshot, live, selectedId, follow, onSelect, onStopFollow, status, mapName, artwork, artworkLoading }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const surface = useRef<SVGSVGElement>(null);
  const drag = useRef<{ id: number; x: number; y: number; cx: number; cy: number; moved: boolean } | null>(null);
  const wheelAction = useRef<(delta: number) => void>(() => undefined);
  const [view, setView] = useState({ zoom: 1, x: 500, y: 500 });
  const [viewport, setViewport] = useState({ width: 1000, height: 1000 });
  const [failedArtwork, setFailedArtwork] = useState<string | null>(null);
  const [failedLink, setFailedLink] = useState<string | null>(null);
  const [fitRevision, setFitRevision] = useState(0);
  // Before a live snapshot arrives a known map can already show its artwork.
  // A snapshot explicitly lacking projection must keep world XY and no overlay.
  const gps = Boolean(snapshot?.projection || !snapshot && artwork);
  const points = useMemo(() => (snapshot?.players ?? []).flatMap(player => {
    const point = arkPlayerPoint(player, snapshot?.projection ?? null);
    return point ? [{ player, ...point }] : [];
  }), [snapshot]);
  // GPS uses its authoritative 0..100 extent. Unknown maps fit only observed XY,
  // never invent GPS calibration or place unspawned players at zero.
  const extent = useMemo(() => {
    if (gps) return { minX: 0, maxX: 100, minY: 0, maxY: 100 };
    const xs = points.map(p => p.x), ys = points.map(p => p.y);
    const midX = xs.length ? (Math.min(...xs) + Math.max(...xs)) / 2 : 0;
    const midY = ys.length ? (Math.min(...ys) + Math.max(...ys)) / 2 : 0;
    const radius = Math.max(5000, ...xs.map(x => Math.abs(x - midX) * 1.2), ...ys.map(y => Math.abs(y - midY) * 1.2));
    return { minX: midX - radius, maxX: midX + radius, minY: midY - radius, maxY: midY + radius };
  }, [gps, snapshot?.mapName, points.length > 0, fitRevision]);
  const normalize = (x: number, y: number) => ({ x: (x - extent.minX) / (extent.maxX - extent.minX) * 1000,
    y: (y - extent.minY) / (extent.maxY - extent.minY) * 1000 });
  const chosen = points.find(point => point.player.playerId === selectedId);
  const center = clampArkMapView({ ...view, ...(follow && chosen && live ? normalize(chosen.x, chosen.y) : {}) }, viewport);
  const size = 1000 / center.zoom, left = center.x - size / 2, top = center.y - size / 2;
  const pixels = Math.max(1, Math.min(viewport.width, viewport.height));
  const unit = size / pixels;
  const art = gps && artwork?.url !== failedArtwork ? artwork : null;
  const sourceUrl = art?.sourceUrl;
  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const measure = () => {
      const { width, height } = node.getBoundingClientRect();
      if (width > 0 && height > 0) setViewport(previous => previous.width === width && previous.height === height ? previous : { width, height });
    };
    measure(); const observer = new ResizeObserver(measure); observer.observe(node);
    return () => observer.disconnect();
  }, []);
  // Keep the stored center bounded too, so resizing back cannot revive an old off-map pan.
  useEffect(() => { setView(previous => clampArkMapView(previous, viewport)); }, [viewport]);
  function zoom(factor: number) { setView(previous => clampArkMapView({ x: center.x, y: center.y, zoom: previous.zoom * factor }, viewport)); }
  wheelAction.current = delta => zoom(delta < 0 ? 1.2 : 1 / 1.2);
  useEffect(() => {
    const node = surface.current;
    function wheel(event: WheelEvent) { event.preventDefault(); wheelAction.current(event.deltaY); }
    node?.addEventListener("wheel", wheel, { passive: false });
    return () => node?.removeEventListener("wheel", wheel);
  }, []);
  function fit() { onStopFollow(); setFitRevision(value => value + 1); setView({ zoom: 1, x: 500, y: 500 }); }
  const tick = (index: number) => index * 100;
  return <div className="ark-map-canvas" data-live={live}>
    <svg ref={surface} className="ark-map-surface" viewBox={`${left} ${top} ${size} ${size}`} tabIndex={0}
      role="group" aria-label={text("方舟地图，方向键平移，加减号缩放，0 适配", "ARK map. Arrow keys pan, plus/minus zoom, 0 fits")}
      onKeyDown={event => {
        if (["+", "=", "-", "0", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) event.preventDefault();
        if (event.key === "+" || event.key === "=") zoom(1.2); else if (event.key === "-") zoom(1 / 1.2); else if (event.key === "0") fit();
        else if (event.key.startsWith("Arrow")) { onStopFollow(); setView(v => clampArkMapView({ ...v, x: center.x + (event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0) * size / 10,
          y: center.y + (event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0) * size / 10 }, viewport)); }
      }}
      onPointerDown={event => { if (event.button !== 0 || (event.target as Element).closest("[data-player-id]")) return;
        drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, cx: center.x, cy: center.y, moved: false };
        event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { const d = drag.current, rect = surface.current?.getBoundingClientRect(); if (!d || !rect) return;
        d.moved = true; onStopFollow(); const length = Math.min(rect.width, rect.height);
        setView(v => clampArkMapView({ ...v, x: d.cx - (event.clientX - d.x) / length * size, y: d.cy - (event.clientY - d.y) / length * size }, viewport)); }}
      onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
      <rect x={left} y={top} width={size} height={size} className="ark-map-background" />
      {art ? <image key={art.url} href={art.url} {...arkArtworkRect(art.bounds)} preserveAspectRatio="none" onError={() => setFailedArtwork(art.url)} /> : null}
      <g className="ark-map-grid" strokeWidth={unit}>
        {Array.from({ length: 11 }, (_, index) => <g key={index}><line x1={tick(index)} x2={tick(index)} y1={0} y2={1000} />
          <line x1={0} x2={1000} y1={tick(index)} y2={tick(index)} /></g>)}
      </g>
      {Array.from({ length: 9 }, (_, i) => i + 1).map(index => <g key={index} className="ark-map-grid-label" fontSize={11 * unit}>
        <text x={tick(index)} y={top + 55 * unit} textAnchor="middle">{(extent.minX + index / 10 * (extent.maxX - extent.minX)).toFixed(0)}</text>
        <text x={left + 8 * unit} y={tick(index)}>{(extent.minY + index / 10 * (extent.maxY - extent.minY)).toFixed(0)}</text>
      </g>)}
      {points.map(point => { const p = normalize(point.x, point.y), selected = point.player.playerId === selectedId;
        return <g key={point.player.playerId} data-player-id={point.player.playerId} role="button" tabIndex={live ? 0 : -1}
          aria-label={`${point.player.name || point.player.playerId} · PlayerID ${point.player.playerId}`} aria-disabled={!live}
          aria-pressed={selected} className="ark-map-player" transform={`translate(${p.x} ${p.y})`}
          onClick={() => { if (live) onSelect(point.player); }} onKeyDown={event => { if (live && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); event.stopPropagation(); onSelect(point.player); } }}>
          <circle r={(selected ? 8 : 6) * unit} strokeWidth={2 * unit} />
          <text x={12 * unit} y={4 * unit} fontSize={13 * unit} strokeWidth={2 * unit}>{point.player.name || point.player.playerId}</text>
        </g>; })}
    </svg>
    <div className="ark-map-controls"><button type="button" className="secondary-button" onClick={() => zoom(1.25)} aria-label={text("放大", "Zoom in")}>+</button>
      <span>{Math.round(view.zoom * 100)}%</span><button type="button" className="secondary-button" onClick={() => zoom(.8)} aria-label={text("缩小", "Zoom out")}>−</button>
      <button type="button" className="secondary-button" onClick={fit}>{text("适配", "Fit")}</button></div>
    <div className="ark-map-caption"><strong>{mapName || "ARK"}</strong><span>{art ? text("地图底图", "Map artwork")
      : artworkLoading ? text("正在加载地图底图…", "Loading map artwork…") : text("坐标网格 · 底图不可用", "Coordinate grid · artwork unavailable")}</span>
      <span>{gps ? "GPS · Lat / Lon" : text("原生 X / Y · 厘米", "Native X / Y · cm")}</span></div>
    <div className="ark-map-state" role="status">{status}</div>
    {art ? <div className="ark-map-source" title={art.source}>{sourceUrl ? <a href={sourceUrl} target="_blank" rel="noopener noreferrer"
      onClick={event => { event.preventDefault(); void openExternalUrl(sourceUrl).catch(() => setFailedLink(sourceUrl)); }}>{art.source}</a> : art.source}
      {sourceUrl && failedLink === sourceUrl ? <span role="status">{text("无法打开来源链接，请复制链接到浏览器。", "Could not open the source. Copy the link into your browser.")}</span> : null}</div> : null}
  </div>;
}
