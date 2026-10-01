import { useEffect, useMemo, useRef, useState } from "react";
import { arkToolsHostAvailable, prepareArkTools, readArkToolsStatus, spawnArkCreature,
  type ArkSpawnResult, type ArkToolsStatus } from "../../api-ark-tools";
import { describeError } from "../../app-state";
import { selectLocaleText, useI18n } from "../../i18n";
import { getArkGmCreatureOptions, localizeArkCatalogName, parseArkEnabledModIds, searchArkGmCreatureOptions, type ArkCustomCreatureEntry } from "./gm-tools";
import "./ark-creature-spawner.css";

interface Props { instanceId: string; moduleId: string; status: string; settingsJson: string }
// These base-game asset paths are shared by ASE and ASA. Other classes can be
// resolved from the loaded world's registry or supplied as a full asset path.
const BASE_CLASSES: Record<string, string> = {
  Rex_Character_BP_C: "/Game/PrimalEarth/Dinos/Rex/Rex_Character_BP.Rex_Character_BP_C",
  Dodo_Character_BP_C: "/Game/PrimalEarth/Dinos/Dodo/Dodo_Character_BP.Dodo_Character_BP_C"
};

function savedCreatures(id: string): ArkCustomCreatureEntry[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(`lsgm.gmTools.arkCreatures.${id}`) ?? "[]");
    if (!Array.isArray(value)) return [];
    return value.filter((entry): entry is ArkCustomCreatureEntry => entry && typeof entry === "object"
      && typeof entry.classId === "string"
      && (entry.label == null || typeof entry.label === "string")
      && (entry.modId == null || typeof entry.modId === "string")).slice(0, 256);
  } catch { return []; }
}

export function ArkCreatureSpawner({ instanceId, moduleId, status, settingsJson }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const scope = useMemo(() => ({ instanceId }), [instanceId]);
  const current = useRef<typeof scope | null>(scope);
  const statusRequest = useRef(0);
  current.current = scope;
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [extension, setExtension] = useState<ArkToolsStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ArkSpawnResult | null>(null);
  const [creature, setCreature] = useState("Rex_Character_BP_C");
  const [level, setLevel] = useState("150");
  const [coordinates, setCoordinates] = useState({ x: "", y: "", z: "" });
  const [tamed, setTamed] = useState(false);
  const [playerId, setPlayerId] = useState("");
  const [search, setSearch] = useState("");
  const [custom, setCustom] = useState(() => savedCreatures(instanceId));
  const [savedLabel, setSavedLabel] = useState("");
  const [savedModId, setSavedModId] = useState("");
  const [page, setPage] = useState(0);
  const host = arkToolsHostAvailable();
  const running = status.toLowerCase() === "running";
  const stopped = status.toLowerCase() === "stopped";
  const enabledMods = useMemo(() => parseArkEnabledModIds(moduleId, settingsJson), [moduleId, settingsJson]);
  const matches = useMemo(() => searchArkGmCreatureOptions(search, getArkGmCreatureOptions().length), [search]);
  const pageCount = Math.max(1, Math.ceil(matches.length / 12));
  const safePage = Math.min(page, pageCount - 1);
  const options = matches.slice(safePage * 12, safePage * 12 + 12);
  const configured = useMemo(() => {
    try { const settings = JSON.parse(settingsJson) as Record<string, unknown>;
      return (settings.rcon_enabled === true || settings.rcon_enabled === "true")
        && typeof settings.admin_password === "string" && Boolean(settings.admin_password.trim());
    } catch { return false; }
  }, [settingsJson]);

  useEffect(() => {
    setCustom(savedCreatures(instanceId)); setCoordinates({ x: "", y: "", z: "" });
    setCreature("Rex_Character_BP_C"); setSearch(""); setPage(0); setSavedLabel(""); setSavedModId(""); setPlayerId("");
    setTamed(false); setLevel("150");
    setResult(null);
  }, [instanceId]);

  function saveCreature() {
    const classId = creature.trim();
    if (!classId || /[\x00-\x1f\x7f]/.test(classId)) {
      setError(text("请输入有效的单行生物类名。", "Enter a valid single-line creature class.")); return;
    }
    const entries = [{ classId, label: savedLabel.trim() || classId, modId: savedModId || null },
      ...custom.filter(entry => entry.classId.toLowerCase() !== classId.toLowerCase())].slice(0, 256);
    try { localStorage.setItem(`lsgm.gmTools.arkCreatures.${instanceId}`, JSON.stringify(entries)); setCustom(entries); setError(null); }
    catch (reason) { setError(describeError(reason)); }
  }

  async function refresh() {
    const token = scope;
    const request = ++statusRequest.current;
    setChecking(true);
    try {
      const value = await readArkToolsStatus(instanceId);
      if (current.current === token && statusRequest.current === request) { setExtension(value); setError(null); }
    } catch (reason) { if (current.current === token && statusRequest.current === request) { setExtension(null); setError(describeError(reason)); } }
    finally { if (current.current === token && statusRequest.current === request) setChecking(false); }
  }
  useEffect(() => {
    current.current = scope;
    setExtension(null); setError(null);
    if (host) void refresh();
    return () => { ++statusRequest.current; if (current.current === scope) current.current = null; };
    // Settings and runtime transitions invalidate the live extension handshake.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, status, settingsJson, host]);

  async function install() {
    if (pending.current || !host || running) return;
    const token = scope;
    pending.current = true; setBusy(true); setError(null);
    try {
      const value = await prepareArkTools(instanceId);
      if (current.current === token) setExtension(value);
    } catch (reason) { if (current.current === token) setError(describeError(reason)); }
    finally { pending.current = false; if (current.current) setBusy(false); }
  }

  async function spawn(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending.current || !running || !extension?.connected || !configured) return;
    const token = scope;
    pending.current = true; setBusy(true); setError(null); setResult(null);
    try {
      const value = await spawnArkCreature({ instanceId, requestId: crypto.randomUUID().replace(/-/g, ""),
        creature: BASE_CLASSES[creature.trim()] ?? creature.trim(), level: Number(level),
        x: Number(coordinates.x), y: Number(coordinates.y), z: Number(coordinates.z), tamed, playerId: tamed ? Number(playerId) : 0 });
      if (current.current === token) setResult(value);
    } catch (reason) { if (current.current === token) setError(describeError(reason)); }
    finally { pending.current = false; if (current.current) setBusy(false); }
  }

  const disabled = busy || checking || !host;
  return <section className="ark-creature-spawner" aria-label={text("生成方舟生物", "Spawn ARK creature")}>
    <div className="ark-creature-spawner__status">
      <p role="status">{!host ? text("请在本机桌面客户端使用方舟生物工具。", "Use ARK creature tools in the local desktop client.")
        : checking ? text("正在检查服务端扩展…", "Checking server extension…")
        : extension?.connected ? text("服务端扩展已连接", "Server extension connected")
        : extension?.installed ? running
          ? text("扩展已安装但未连接，请检查运行日志后重试连接。", "Extension installed but disconnected. Check the runtime log and retry the connection.")
          : text("扩展已安装，启动实例后可生成生物。", "Extension installed. Start the instance to spawn creatures.")
        : text("首次使用需要在停服时安装生物工具扩展。", "Install the creature extension while the instance is stopped before first use.")}</p>
      <button type="button" className="secondary-button" disabled={disabled} onClick={() => void refresh()}>{text("检查连接", "Check connection")}</button>
    </div>
    {!extension?.installed && host ? <div className="ark-creature-spawner__install">
      <p className="ark-creature-spawner__note">{text("将从 ArkServerApi 下载已校验版本，仅安装到本实例的独立程序目录；已有第三方加载器冲突时会停止。", "Download a verified ArkServerApi release into this instance's private program directory. An existing conflicting loader stops installation.")}</p>
      {moduleId === "arksurvivalascended" ? <p className="ark-creature-spawner__note">{text("安装后，扩展在首次启动或游戏更新时会向 cdn.pelayori.com（备用 cdn.shadowhunter.co.za / cdn.shadowhunter-systems.co.za）发送服务器程序 SHA-256 哈希，下载匹配文件。点击安装即允许此下载。", "On first startup or game updates, the extension sends the server EXE SHA-256 hash to cdn.pelayori.com (fallbacks: cdn.shadowhunter.co.za / cdn.shadowhunter-systems.co.za) for matching files. Installing allows these downloads.")}</p> : null}
      <button type="button" className="secondary-button" disabled={disabled || !stopped} onClick={() => void install()}>{busy ? text("正在安装…", "Installing…") : text("安装服务端扩展", "Install server extension")}</button>
      {!stopped ? <p className="ark-creature-spawner__note">{text("请先停止实例。", "Stop the instance first.")}</p> : null}
    </div> : null}
    {extension?.issue ? <p className="ark-creature-spawner__note" role="status">{extension.issue}</p> : null}
    {running && !configured ? <p className="ark-creature-spawner__note">{text("请先启用 RCON 并设置管理员密码。", "Enable RCON and configure the admin password first.")}</p> : null}
    <form onSubmit={event => void spawn(event)}>
      <fieldset disabled={busy}>
        <label className="gmt-field"><span>{text("查找生物", "Find creature")}</span><input className="gmt-text-input" type="search" value={search} placeholder={text("输入名称或类名", "Search names or classes")} onChange={e => { setSearch(e.target.value); setPage(0); }} /></label>
        <div className="ark-creature-spawner__catalog-nav"><span>{text(`匹配 ${matches.length} 种生物`, `${matches.length} matching creatures`)}</span><button type="button" className="secondary-button" disabled={safePage === 0} onClick={() => setPage(safePage - 1)}>{text("上一页", "Previous")}</button><span>{safePage + 1}/{pageCount}</span><button type="button" className="secondary-button" disabled={safePage + 1 === pageCount} onClick={() => setPage(safePage + 1)}>{text("下一页", "Next")}</button></div>
        <div className="ark-creature-spawner__choices">{options.map(option => <button key={option.value} type="button" className="secondary-button" aria-pressed={creature === option.value} onClick={() => setCreature(option.value)}>{localizeArkCatalogName(option, locale)} <small>{option.value}</small></button>)}</div>
        <label className="gmt-field"><span>{text("生物类名或完整路径", "Creature class or full path")}</span><input className="gmt-text-input" required maxLength={300} value={creature} onChange={e => setCreature(e.target.value)} /></label>
        <div className="ark-creature-spawner__saved"><label className="gmt-field"><span>{text("收藏名称", "Saved name")}</span><input className="gmt-text-input" value={savedLabel} maxLength={120} onChange={e => setSavedLabel(e.target.value)} /></label>{enabledMods.length ? <label className="gmt-field"><span>Mod</span><select className="gmt-select" value={savedModId} onChange={e => setSavedModId(e.target.value)}><option value="">{text("不关联 Mod", "No mod tag")}</option>{enabledMods.map(id => <option key={id} value={id}>{id}</option>)}</select></label> : null}<button type="button" className="secondary-button" onClick={saveCreature}>{text("收藏生物", "Save creature")}</button></div>
        {custom.length ? <div className="ark-creature-spawner__choices">{custom.map(entry => <button key={entry.classId} type="button" className="secondary-button" onClick={() => setCreature(entry.classId)}>{entry.label || entry.classId}<small>{entry.classId}{entry.modId ? ` · Mod ${entry.modId}` : ""}</small></button>)}</div> : null}
        <p className="ark-creature-spawner__note">{text("当前地图未加载的生物需要完整 /Game/…_C 类路径；Mod 生物需先安装并启用对应 Mod。", "Creatures not loaded by this map need a full /Game/…_C class path. Mod creatures require the corresponding installed and enabled mod.")}</p>
        <div className="ark-creature-spawner__coordinates">{(["x", "y", "z"] as const).map(axis => <label className="gmt-field" key={axis}><span>{text("世界坐标", "World coordinate")} {axis.toUpperCase()}</span><input className="gmt-text-input" type="number" required min={-10000000} max={10000000} step="any" value={coordinates[axis]} onChange={e => setCoordinates(previous => ({ ...previous, [axis]: e.target.value }))} /></label>)}</div>
        <p className="ark-creature-spawner__note">{text("使用游戏世界 X/Y/Z 坐标（厘米），不是地图经纬度。请填写已知安全位置。", "Use game-world X/Y/Z coordinates in centimeters, not map latitude/longitude. Enter a known safe position.")}</p>
        <div className="ark-creature-spawner__coordinates"><label className="gmt-field"><span>{text("基础等级", "Base level")}</span><input className="gmt-text-input" type="number" required min={1} max={5000} step={1} value={level} onChange={e => setLevel(e.target.value)} /></label>
          <label className="gmt-field"><span>{text("生成状态", "Creature state")}</span><select className="gmt-select" value={String(tamed)} onChange={e => setTamed(e.target.value === "true")}><option value="false">{text("野生", "Wild")}</option><option value="true">{text("驯服并归属玩家", "Tamed, owned by player")}</option></select></label>
          {tamed ? <label className="gmt-field"><span>{text("在线玩家的游戏内 ID", "Online in-game Player ID")}</span><input className="gmt-text-input" required type="number" min={1} max={4294967295} step={1} value={playerId} onChange={e => setPlayerId(e.target.value)} /></label> : null}</div>
        <button type="submit" className="primary-button" disabled={disabled || !running || !configured || !extension?.connected}>{busy ? text("正在生成并核验…", "Spawning and verifying…") : text("生成生物", "Spawn creature")}</button>
      </fieldset>
    </form>
    {error ? <p className="ark-creature-spawner__error" role="alert">{error}</p> : null}
    {result?.instanceId === instanceId ? <div className="ark-creature-spawner__result" role="status"><strong>{text("已生成并读回确认", "Spawned and verified")}</strong><p>{result.creature.className} · {text("实际等级", "Actual level")} {result.creature.level} · ID {result.creature.id1}:{result.creature.id2}</p><p>X {result.creature.x.toFixed(1)} · Y {result.creature.y.toFixed(1)} · Z {result.creature.z.toFixed(1)}</p></div> : null}
  </section>;
}
