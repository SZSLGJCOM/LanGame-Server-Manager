import { DST_MAP_MAX_ZOOM, DST_MAP_MIN_ZOOM, DST_MAP_ZOOM_PRESETS, clampDstMapZoom, stepDstMapZoom } from "../../dst-map-view";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";

interface Props { zoom: number; onChange: (zoom: number) => void; onFit: () => void; maximum?: number }

/** Compact zoom overlay floating in the map's top-left corner; it never starts a map drag. */
export function DstMapZoomControls({ zoom, onChange, onFit, maximum = DST_MAP_MAX_ZOOM }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const percent = Math.round(zoom * 100);
  const limit = clampDstMapZoom(maximum, maximum);
  const presets = [...DST_MAP_ZOOM_PRESETS, 16, 32, 64, 128, 256, 512].filter(value => value <= limit);
  const preset = presets.some(value => value === zoom);
  return <div className="dst-map-controls" role="group" aria-label={text("缩放", "Zoom")} onPointerDown={event => event.stopPropagation()}
    onDoubleClick={event => event.stopPropagation()} onKeyDown={event => event.stopPropagation()}>
    <button type="button" className="secondary-button" aria-label={text("缩小", "Zoom out")} title={text("缩小", "Zoom out")} disabled={zoom <= DST_MAP_MIN_ZOOM}
      onClick={() => onChange(stepDstMapZoom(zoom, -1, limit))}><ShellIcon name="minus" /></button>
    <select className="settings-schema-select dst-map-zoom" aria-label={text("地图缩放", "Map zoom")} value={String(zoom)}
      onChange={event => onChange(clampDstMapZoom(Number(event.target.value), limit))}>
      {!preset ? <option value={String(zoom)}>{percent}%</option> : null}
      {presets.map(value => <option key={value} value={String(value)}>{value * 100}%</option>)}
      {!presets.includes(limit) && limit !== zoom ? <option value={String(limit)}>{Math.round(limit * 100)}%</option> : null}
    </select>
    <button type="button" className="secondary-button" aria-label={text("放大", "Zoom in")} title={text("放大", "Zoom in")} disabled={zoom >= limit}
      onClick={() => onChange(stepDstMapZoom(zoom, 1, limit))}><ShellIcon name="plus" /></button>
    <button type="button" className="secondary-button" aria-label={text("适应窗口", "Fit to view")} title={text("适应窗口", "Fit to view")} onClick={onFit}><ShellIcon name="fit" /></button>
  </div>;
}
