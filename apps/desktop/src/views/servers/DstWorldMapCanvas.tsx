import { useEffect, useRef, useState, type ReactNode } from "react";
import { mapPlayerArtwork, mapPlayerPoint, type DstMapClick, type DstWorldMap } from "../../dst-world-map";
import { renderDstTerrain } from "../../dst-map-renderer";
import { ActivityNotice } from "../../components/ActivityNotice";
import { selectLocaleText, useI18n } from "../../i18n";
import { DstMapLandmarks } from "./DstMapLandmarks";
import { anchoredDstMapCenter, clampDstMapCenter, clampDstMapZoom, dstMapFitScale, dstMapZoomLimit, stepDstMapZoom } from "../../dst-map-view";

interface Props {
  snapshot: DstWorldMap; selectedPlayer: string | null; zoom: number;
  fitRevision: number; onZoomChange: (zoom: number) => void; onCancelFollow: () => void; onFit: () => void;
  onZoomLimitChange?: (maximum: number) => void;
  /** A click (not a drag) on a player marker selects that player. */
  /** A click that did not drag: the hit player, if any, and the map point (canvas pixels, y down). */
  onMapClick?: (click: DstMapClick) => void;
  /** Overlay pinned to the map's top-left corner, e.g. zoom controls. */
  controls?: ReactNode;
  initialCenter?: Point; onCenterChange?: (point: Point) => void;
  renderDetails?: (content: ReactNode) => ReactNode;
}
interface Point { x: number; y: number }
interface Drag { pointerId: number; start: Point; center: Point; moved: boolean }

export function DstWorldMapCanvas({ snapshot, selectedPlayer, zoom, fitRevision, onZoomChange, onCancelFollow, onFit,
  onZoomLimitChange, onMapClick, controls, initialCenter, onCenterChange, renderDetails }: Props) {
  const { locale } = useI18n();
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [center, setCenter] = useState<Point>(() => ({
    x: initialCenter && Number.isFinite(initialCenter.x) ? Math.max(0, Math.min(snapshot.width, initialCenter.x)) : snapshot.width / 2,
    y: initialCenter && Number.isFinite(initialCenter.y) ? Math.max(0, Math.min(snapshot.height, initialCenter.y)) : snapshot.height / 2,
  }));
  const [dragging, setDragging] = useState(false);
  const drag = useRef<Drag | null>(null);
  const touches = useRef(new Map<number, Point>());
  const pinch = useRef<{ distance: number; midpoint: Point } | null>(null);
  const [hover, setHover] = useState("");
  const [renderError, setRenderError] = useState<string | null>(null);
  const [unavailableTextures, setUnavailableTextures] = useState<string[]>([]);
  const [iconFailures, setIconFailures] = useState<{ sources: DstWorldMap["art"]; names: Set<string> }>({ sources: null, names: new Set() });
  const unavailableIcons = iconFailures.sources === snapshot.art ? iconFailures.names : new Set<string>();
  function iconFailed(icon: string) {
    setIconFailures(previous => {
      const names = previous.sources === snapshot.art ? previous.names : new Set<string>();
      if (names.has(icon) || names.size >= 512) return previous;
      return { sources: snapshot.art, names: new Set([...names, icon]) };
    });
  }
  const [rendering, setRendering] = useState(false);
  const fitScale = dstMapFitScale(snapshot.width, snapshot.height, size.width, size.height);
  const maximumZoom = dstMapZoomLimit(snapshot.width, snapshot.height, size.width, size.height);
  const view = useRef({ center, zoom, fitScale, maximumZoom, width: snapshot.width, height: snapshot.height, size });
  view.current = { center, zoom, fitScale, maximumZoom, width: snapshot.width, height: snapshot.height, size };
  const callbacks = useRef({ onZoomChange, onCancelFollow, onCenterChange, onMapClick, players: snapshot.players });
  callbacks.current = { onZoomChange, onCancelFollow, onCenterChange, onMapClick, players: snapshot.players };
  const previousFit = useRef({ revision: fitRevision, width: snapshot.width, height: snapshot.height });
  function moveCenter(point: Point, scale = view.current.fitScale * view.current.zoom) {
    const next = clampDstMapCenter(point, view.current, view.current.size, scale);
    if (next.x === view.current.center.x && next.y === view.current.center.y) return;
    view.current.center = next;
    setCenter(next);
    callbacks.current.onCenterChange?.(next);
  }
  function zoomAt(value: number, point?: Point) {
    const current = view.current, node = viewport.current;
    const nextZoom = clampDstMapZoom(value, current.maximumZoom);
    if (!node || !(current.fitScale > 0) || nextZoom === current.zoom) return;
    const bounds = node.getBoundingClientRect();
    const offset = point ? { x: point.x - bounds.left - node.clientLeft - node.clientWidth / 2,
      y: point.y - bounds.top - node.clientTop - node.clientHeight / 2 } : { x: 0, y: 0 };
    callbacks.current.onCancelFollow();
    drag.current = null; setDragging(false); setHover("");
    moveCenter(anchoredDstMapCenter(current.center, offset, current.fitScale * current.zoom, current.fitScale * nextZoom), current.fitScale * nextZoom);
    view.current.zoom = nextZoom;
    callbacks.current.onZoomChange(nextZoom);
  }
  function clickAt(point: Point): DstMapClick | null {
    const current = view.current, node = viewport.current, scale = current.fitScale * current.zoom;
    if (!node || !(scale > 0)) return null;
    const bounds = node.getBoundingClientRect();
    const x = current.center.x + (point.x - bounds.left - node.clientLeft - node.clientWidth / 2) / scale;
    const y = current.center.y + (point.y - bounds.top - node.clientTop - node.clientHeight / 2) / scale;
    let playerId: string | null = null, distance = 16 / scale;
    for (const player of callbacks.current.players) {
      const [px, py] = mapPlayerPoint(player, current.height), next = Math.hypot(px - x, py - y);
      if (next <= distance) { playerId = player.id; distance = next; }
    }
    return { playerId, x, y };
  }
  function touchGeometry() {
    const points = [...touches.current.values()];
    return points.length === 2 ? { distance: Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y),
      midpoint: { x: (points[0].x + points[1].x) / 2, y: (points[0].y + points[1].y) / 2 } } : null;
  }
  useEffect(() => {
    const node = viewport.current;
    if (!node) return;
    const measure = () => setSize(previous => previous.width === node.clientWidth && previous.height === node.clientHeight
      ? previous : { width: node.clientWidth, height: node.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    drag.current = null; touches.current.clear(); pinch.current = null; setDragging(false); setHover("");
    const previous = previousFit.current;
    if (previous.revision === fitRevision && previous.width === snapshot.width && previous.height === snapshot.height) return;
    previousFit.current = { revision: fitRevision, width: snapshot.width, height: snapshot.height };
    moveCenter({ x: snapshot.width / 2, y: snapshot.height / 2 });
  }, [fitRevision, snapshot.width, snapshot.height]);
  useEffect(() => {
    if (size.width <= 0 || size.height <= 0) return;
    onZoomLimitChange?.(maximumZoom);
    const nextZoom = clampDstMapZoom(view.current.zoom, maximumZoom);
    if (nextZoom !== view.current.zoom) { view.current.zoom = nextZoom; callbacks.current.onZoomChange(nextZoom); }
  }, [maximumZoom, onZoomLimitChange, size.width, size.height]);
  useEffect(() => {
    const player = snapshot.players.find(value => value.id === selectedPlayer);
    if (!player) return;
    const [x, y] = mapPlayerPoint(player, snapshot.height);
    moveCenter({ x, y });
  }, [selectedPlayer, snapshot.players, snapshot.height]);
  useEffect(() => {
    const node = viewport.current;
    if (!node) return;
    const wheel = (event: WheelEvent) => {
      if (!view.current.fitScale) return;
      event.preventDefault();
      const current = view.current;
      const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? node.clientHeight : 1);
      zoomAt(current.zoom * Math.exp(-Math.max(-500, Math.min(500, delta)) * .0015), { x: event.clientX, y: event.clientY });
    };
    const endDrag = () => { drag.current = null; touches.current.clear(); pinch.current = null; setDragging(false); };
    const pointerMove = (event: PointerEvent) => {
      if (touches.current.has(event.pointerId)) {
        touches.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
        const next = touchGeometry(), previous = pinch.current;
        if (next && previous && next.distance > 0 && previous.distance > 0) {
          const current = view.current, bounds = node.getBoundingClientRect();
          const offset = { x: previous.midpoint.x - bounds.left - node.clientLeft - node.clientWidth / 2,
            y: previous.midpoint.y - bounds.top - node.clientTop - node.clientHeight / 2 };
          const nextZoom = clampDstMapZoom(current.zoom * next.distance / previous.distance, current.maximumZoom);
          const nextScale = current.fitScale * nextZoom;
          if (!(nextScale > 0)) return;
          const anchored = anchoredDstMapCenter(current.center, offset, current.fitScale * current.zoom, nextScale);
          callbacks.current.onCancelFollow(); setHover("");
          moveCenter({ x: anchored.x + (previous.midpoint.x - next.midpoint.x) / nextScale,
            y: anchored.y + (previous.midpoint.y - next.midpoint.y) / nextScale }, nextScale);
          view.current.zoom = nextZoom; callbacks.current.onZoomChange(nextZoom);
          pinch.current = next; return;
        }
      }
      const active = drag.current;
      if (!active || active.pointerId !== event.pointerId) return;
      if (!(event.buttons & 1)) { endDrag(); return; }
      const dx = event.clientX - active.start.x, dy = event.clientY - active.start.y;
      if (!active.moved && Math.hypot(dx, dy) < 3) return;
      active.moved = true;
      callbacks.current.onCancelFollow(); setDragging(true); setHover("");
      const scale = view.current.fitScale * view.current.zoom;
      if (scale > 0) moveCenter({ x: active.center.x - dx / scale, y: active.center.y - dy / scale });
    };
    const pointerEnd = (event: PointerEvent) => {
      if (touches.current.delete(event.pointerId)) {
        pinch.current = null;
        const remaining = [...touches.current.entries()];
        drag.current = remaining.length === 1 ? { pointerId: remaining[0][0], start: remaining[0][1], center: view.current.center, moved: false } : null;
        setDragging(false); return;
      }
      const active = drag.current;
      if (active?.pointerId !== event.pointerId) return;
      const picked = active.moved ? null : clickAt({ x: event.clientX, y: event.clientY });
      endDrag();
      if (picked) callbacks.current.onMapClick?.(picked);
    };
    node.addEventListener("wheel", wheel, { passive: false });
    // Window ownership keeps a drag coherent when the pointer leaves the map.
    window.addEventListener("pointermove", pointerMove);
    window.addEventListener("pointerup", pointerEnd);
    window.addEventListener("pointercancel", pointerEnd);
    window.addEventListener("blur", endDrag);
    return () => {
      node.removeEventListener("wheel", wheel);
      window.removeEventListener("pointermove", pointerMove);
      window.removeEventListener("pointerup", pointerEnd);
      window.removeEventListener("pointercancel", pointerEnd);
      window.removeEventListener("blur", endDrag);
      drag.current = null; touches.current.clear(); pinch.current = null;
    };
  }, []);
  useEffect(() => {
    let disposed = false;
    const controller = new AbortController();
    setRendering(true); setRenderError(null); setUnavailableTextures([]);
    const node = canvas.current;
    node?.getContext("2d")?.clearRect(0, 0, node.width, node.height);
    void renderDstTerrain(snapshot, controller.signal, paths => { if (!disposed) setUnavailableTextures(paths); }).then(rendered => {
      if (disposed) return;
      const context = canvas.current?.getContext("2d");
      if (!context) throw new Error("World map canvas is unavailable.");
      context.drawImage(rendered, 0, 0);
    }).catch(cause => {
      if (!disposed && !controller.signal.aborted) setRenderError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      if (!disposed) setRendering(false);
    });
    return () => { disposed = true; controller.abort(); };
    // Position updates reuse the same terrain and decoded artwork references.
  }, [snapshot.tiles, snapshot.width, snapshot.height, snapshot.tileNames, snapshot.tileKinds, snapshot.tileTextures, snapshot.tileBaseTextures, snapshot.tileColors, snapshot.terrainLayers, snapshot.roads, snapshot.art]);
  const scale = fitScale * zoom;
  // Resizing, refitting or a parent zoom change re-clamps the stored center as well as the drawn one.
  const shown = clampDstMapCenter(center, snapshot, size, scale);
  // Clamp the latest stored center, not this render's, so a same-commit fit or focus is never overwritten.
  useEffect(() => { if (shown.x !== center.x || shown.y !== center.y) moveCenter(view.current.center, scale); }, [shown.x, shown.y, center.x, center.y, scale]);
  const markerSize = scale > 0 ? 12 / scale : 0;
  const label = selectLocaleText(locale, "服务器世界地形与玩家位置", "Server world terrain and player positions");
  const mapViewport = (
    <div ref={viewport} className={`dst-map-viewport${dragging ? " is-dragging" : ""}`} tabIndex={0} role="region" aria-label={label}
      aria-busy={rendering} aria-keyshortcuts="+ - ArrowUp ArrowDown ArrowLeft ArrowRight Home 0"
      onDoubleClick={event => {
        event.preventDefault();
        zoomAt(stepDstMapZoom(view.current.zoom, event.shiftKey ? -1 : 1, view.current.maximumZoom), { x: event.clientX, y: event.clientY });
      }}
      onPointerDown={event => {
        if (event.button !== 0 || !view.current.fitScale) return;
        event.preventDefault(); event.currentTarget.focus({ preventScroll: true });
        if (event.pointerType === "touch") {
          if (touches.current.size >= 2) return;
          touches.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
          if (touches.current.size === 2) { pinch.current = touchGeometry(); drag.current = null; setDragging(false); return; }
        }
        drag.current = { pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, center: view.current.center, moved: false };
      }}
      onKeyDown={event => {
        if (event.altKey || event.ctrlKey || event.metaKey) return;
        if (["+", "=", "-", "_"].includes(event.key)) {
          event.preventDefault(); zoomAt(stepDstMapZoom(view.current.zoom, ["+", "="].includes(event.key) ? 1 : -1, view.current.maximumZoom));
        } else if (event.key === "Home" || event.key === "0") {
          event.preventDefault(); onFit();
        } else if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key) && scale > 0) {
          event.preventDefault(); onCancelFollow();
          const step = (event.shiftKey ? 160 : 64) / scale;
          moveCenter({ x: center.x + (event.key === "ArrowRight" ? step : event.key === "ArrowLeft" ? -step : 0),
            y: center.y + (event.key === "ArrowDown" ? step : event.key === "ArrowUp" ? -step : 0) });
        }
      }}>
      <div className="dst-map-plane" style={{ width: snapshot.width * scale, height: snapshot.height * scale,
        transform: `translate3d(${size.width / 2 - shown.x * scale}px, ${size.height / 2 - shown.y * scale}px, 0)` }}>
        <canvas ref={canvas} width={snapshot.width * 4} height={snapshot.height * 4} role="img" aria-label={label}
          onPointerLeave={() => setHover("")}
          onPointerMove={event => {
            if (drag.current?.moved) return;
            const bounds = event.currentTarget.getBoundingClientRect();
            const x = Math.floor((event.clientX - bounds.left) / bounds.width * snapshot.width);
            const z = snapshot.height - 1 - Math.floor((event.clientY - bounds.top) / bounds.height * snapshot.height);
            if (x < 0 || x >= snapshot.width || z < 0 || z >= snapshot.height) { setHover(""); return; }
            const tile = snapshot.tiles?.[z * snapshot.width + x];
            setHover(tile === undefined ? "" : `${snapshot.tileNames[String(tile)] ?? tile} · ${x}, ${z}`);
          }} />
        {scale > 0 ? <DstMapLandmarks snapshot={snapshot} view={{ centerX: shown.x, centerZ: snapshot.height - shown.y, width: size.width, height: size.height, scale }}
          unavailableIcons={unavailableIcons} onArtworkFailure={iconFailed} /> : null}
        <svg viewBox={`0 0 ${snapshot.width} ${snapshot.height}`} className="dst-map-players" aria-hidden="true">
          {snapshot.players.map(player => {
            const [x, y] = mapPlayerPoint(player, snapshot.height);
            const selected = selectedPlayer === player.id;
            const avatar = player.icon !== null && !unavailableIcons.has(player.icon)
              ? mapPlayerArtwork(snapshot.art, player.icon) : undefined;
            return <g key={player.id} transform={`translate(${x} ${y})`}>
              <circle r={markerSize * (selected ? 1.2 : 1)} fill={selected ? "#ffdb80" : "#f7fafc"} stroke="#17202b" strokeWidth={markerSize * .15} />
              {avatar ? <image href={avatar} x={-markerSize} y={-markerSize} width={markerSize * 2} height={markerSize * 2}
                onError={() => { if (player.icon !== null) iconFailed(player.icon); }} /> : null}
              <text x={markerSize * 1.4} y={markerSize * .4} fontSize={markerSize} paintOrder="stroke" stroke="#17202b" strokeWidth={markerSize * .2} fill="#fff">{player.name || player.id}</text>
            </g>;
          })}
        </svg>
      </div>
      {controls}
      {/* Always mounted in a map corner so hovering never shifts the layout; an em dash means off-map. */}
      <output className="dst-map-readout" aria-live="off">{hover || "—"}</output>
    </div>
  );
  const details = <>
    {renderError ? <ActivityNotice tone="error">{`${selectLocaleText(locale, "地图绘制未完成", "Map rendering did not complete")}: ${renderError}`}</ActivityNotice> : null}
    {unavailableTextures.length ? <details className="dst-map-art-issues dst-map-render-issues">
      <summary>{selectLocaleText(locale, "部分纹理未能解码，相关地块暂用简图显示", "Some textures could not be decoded; affected terrain uses schematic colors")}</summary>
      <ul>{unavailableTextures.map(path => <li key={path}>{path}</li>)}</ul>
    </details> : null}
    {unavailableIcons.size ? <details className="dst-map-art-issues dst-map-icon-issues">
      <summary>{selectLocaleText(locale, "部分图标未能解码，已保留位置标记", "Some icons could not be decoded; position markers are retained")}</summary>
      <ul>{[...unavailableIcons].sort().map(icon => <li key={icon}>{icon}</li>)}</ul>
    </details> : null}

  </>;
  return <>{mapViewport}{renderDetails ? renderDetails(details) : details}</>;
}
