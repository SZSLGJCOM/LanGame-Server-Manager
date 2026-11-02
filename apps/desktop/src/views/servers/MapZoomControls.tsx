import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import "./map-zoom-controls.css";

const ZOOM_PRESETS = [1, 2, 4, 8, 10, 16, 32, 64, 128, 256, 512];

interface Props { zoom: number; onChange: (zoom: number) => void; onFit: () => void; minimum?: number; maximum?: number }

/** Compact zoom overlay floating in the map's top-left corner; it never starts a map drag. */
export function MapZoomControls({ zoom, onChange, onFit, minimum = 1, maximum = 10 }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const percent = Math.round(zoom * 100);
  const floor = Number.isFinite(minimum) && minimum > 0 ? minimum : 1;
  const limit = Math.max(floor, Number.isFinite(maximum) ? maximum : 10);
  const clamp = (value: number) => Math.max(floor, Math.min(limit, Number.isFinite(value) ? value : floor));
  const presets = [...new Set([floor, ...ZOOM_PRESETS.filter(value => value >= floor && value <= limit), limit])];
  const preset = presets.some(value => value === zoom);
  return <div className="map-zoom-controls" role="group" aria-label={text("缩放", "Zoom")} onPointerDown={event => event.stopPropagation()}
    onDoubleClick={event => event.stopPropagation()} onKeyDown={event => event.stopPropagation()}>
    <button type="button" className="secondary-button" aria-label={text("缩小", "Zoom out")} title={text("缩小", "Zoom out")} disabled={zoom <= floor}
      onClick={() => onChange(clamp(zoom * .8))}><ShellIcon name="minus" /></button>
    <select className="settings-schema-select map-zoom-select" aria-label={text("地图缩放", "Map zoom")} value={String(zoom)}
      onChange={event => onChange(clamp(Number(event.target.value)))}>
      {!preset ? <option value={String(zoom)}>{percent}%</option> : null}
      {presets.map(value => <option key={value} value={String(value)}>{Math.round(value * 100)}%</option>)}
    </select>
    <button type="button" className="secondary-button" aria-label={text("放大", "Zoom in")} title={text("放大", "Zoom in")} disabled={zoom >= limit}
      onClick={() => onChange(clamp(zoom * 1.25))}><ShellIcon name="plus" /></button>
    <button type="button" className="secondary-button" aria-label={text("适应窗口", "Fit to view")} title={text("适应窗口", "Fit to view")} onClick={onFit}><ShellIcon name="fit" /></button>
  </div>;
}
