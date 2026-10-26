import { useEffect, useRef, useState } from "react";
import { ArkArtworkInvalidError, importArkArtwork, readArkImportedArtwork, removeArkImportedArtwork, type ArkImportedArtwork } from "../../ark-map-artwork";
import { describeError } from "../../app-state";
import { selectLocaleText, useI18n } from "../../i18n";
import { ActivityNotice } from "../../components/ActivityNotice";

interface Props { edition: string; mapName: string; calibrated: boolean; onChange: (value: ArkImportedArtwork | null) => void }
export function ArkMapArtworkControl({ edition, mapName, calibrated, onChange }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const input = useRef<HTMLInputElement>(null);
  const [artwork, setArtwork] = useState<ArkImportedArtwork | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [corrupt, setCorrupt] = useState(false);
  const callback = useRef(onChange); callback.current = onChange;
  const scope = `${edition}:${mapName.toLowerCase()}`;
  const current = useRef<string | null>(scope); current.current = scope;
  useEffect(() => {
    let active = true;
    current.current = scope;
    setArtwork(null); setError(null); setBusy(false); setCorrupt(false);
    if (mapName) void readArkImportedArtwork(edition, mapName).then(value => {
      if (active) { setArtwork(value); callback.current(value); }
    }).catch(reason => { if (active) { setError(describeError(reason)); setCorrupt(reason instanceof ArkArtworkInvalidError); } });
    return () => { active = false; current.current = null; };
  }, [edition, mapName]);
  async function change(file: File | null) {
    if (busy || !mapName || file && !calibrated) return;
    const token = scope; setBusy(true); setError(null);
    try {
      const next = file ? await importArkArtwork(edition, mapName, file) : (await removeArkImportedArtwork(edition, mapName), null);
      if (current.current === token) { setArtwork(next); setCorrupt(false); callback.current(next); }
    } catch (reason) { if (current.current === token) setError(describeError(reason)); }
    finally { if (current.current === token) setBusy(false); }
  }
  return <details className="ark-map-artwork-control"><summary>{text("地图底图", "Map artwork")}{artwork ? ` · ${text("本地导入", "Imported locally")}` : ""}</summary>
    <p>{text("常用地图会按游戏版本自动加载并缓存底图。中文优先国内镜像，英文优先海外来源；失败时自动切源，均不可用时显示坐标网格。",
      "Common maps automatically load and cache matching artwork. Chinese prefers the domestic mirror; English prefers the international source. Unavailable sources switch automatically, with a coordinate grid as the final fallback.")}</p>
    <p>{text("自定义地图也可使用本机 PNG 覆盖：北朝上、无边框、经纬度 0–100，正方形 128–4096 像素，最大 4 MiB。移除后恢复自动底图。",
      "For a custom map, optionally override with a local north-up, borderless PNG covering GPS 0–100 (square, 128–4096 pixels, up to 4 MiB). Removing it restores automatic artwork.")}</p>
    {!calibrated ? <p>{text("当前地图缺少可靠的坐标投影，保留原生 XY 网格。", "This map has no reliable projection. Native XY grid is retained.")}</p> : null}
    <input ref={input} type="file" accept="image/png,.png" hidden onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void change(file); }} />
    <button className="secondary-button" type="button" disabled={busy || !calibrated || !mapName} onClick={() => input.current?.click()}>{text("导入 PNG", "Import PNG")}</button>
    {artwork || corrupt ? <button className="secondary-button" type="button" disabled={busy} onClick={() => void change(null)}>{text("移除底图", "Remove artwork")}</button> : null}
    {error ? <ActivityNotice tone="warning">{error}</ActivityNotice> : null}
  </details>;
}
