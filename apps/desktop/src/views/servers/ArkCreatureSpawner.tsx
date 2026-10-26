import { useEffect, useMemo, useRef, useState } from "react";
import { arkToolsHostAvailable, prepareArkTools, readArkToolsStatus, spawnArkCreature,
  type ArkSpawnResult, type ArkToolsStatus } from "../../api-ark-tools";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ConfigurationActionHelp, ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import { ShellIcon } from "../../components/ShellIcon";
import { describeError } from "../../app-state";
import { selectLocaleText, useI18n } from "../../i18n";
import { parseArkEnabledModIds, type ArkCustomCreatureEntry } from "./gm-tools";
import { ArkCatalogPicker } from "./ArkCatalogPicker";
import type { ArkMapPlayer } from "../../ark-world-map";
import "./ark-creature-spawner.css";

interface Props { instanceId: string; moduleId: string; status: string; settingsJson: string; processKey: string;
  runId: number | null; runtimeRunning?: boolean; selectedPlayer?: ArkMapPlayer | null }
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

export function ArkCreatureSpawner({ instanceId, moduleId, status, settingsJson, processKey, runId, runtimeRunning, selectedPlayer }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const scope = useMemo(() => ({ instanceId, processKey, runId, status, runtimeRunning }), [instanceId, processKey, runId, status, runtimeRunning]);
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
  const [sourcePlayerId, setSourcePlayerId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [custom, setCustom] = useState(() => savedCreatures(instanceId));
  const [savedLabel, setSavedLabel] = useState("");
  const [savedModId, setSavedModId] = useState("");
  const host = arkToolsHostAvailable();
  const running = runtimeRunning ?? status.toLowerCase() === "running";
  const stopped = status.toLowerCase() === "stopped";
  const enabledMods = useMemo(() => parseArkEnabledModIds(moduleId, settingsJson), [moduleId, settingsJson]);
  const configured = useMemo(() => {
    try { const settings = JSON.parse(settingsJson) as Record<string, unknown>;
      return (settings.rcon_enabled === true || settings.rcon_enabled === "true")
        && typeof settings.admin_password === "string" && Boolean(settings.admin_password.trim());
    } catch { return false; }
  }, [settingsJson]);
  useEffect(() => setResult(null), [scope]);

  useEffect(() => {
    setCustom(savedCreatures(instanceId)); setCoordinates({ x: "", y: "", z: "" });
    setCreature("Rex_Character_BP_C"); setSearch(""); setSavedLabel(""); setSavedModId(""); setPlayerId("");
    setTamed(false); setLevel("150");
    setResult(null);
    setSourcePlayerId(null);
  }, [instanceId, processKey, runId]);
  useEffect(() => {
    if (sourcePlayerId !== null && selectedPlayer?.playerId === sourcePlayerId && selectedPlayer.x !== null && selectedPlayer.y !== null && selectedPlayer.z !== null) {
      setCoordinates({ x: String(selectedPlayer.x), y: String(selectedPlayer.y), z: String(selectedPlayer.z) });
      setPlayerId(String(selectedPlayer.playerId));
    }
  }, [sourcePlayerId, selectedPlayer]);

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
      const value = await readArkToolsStatus(instanceId, processKey);
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
    if (pending.current || !running || !runId || !extension?.connected || !configured || stalePlayer) return;
    const token = scope;
    pending.current = true; setBusy(true); setError(null); setResult(null);
    try {
      const value = await spawnArkCreature({ instanceId, processKey, runId, requestId: crypto.randomUUID().replace(/-/g, ""),
        creature: BASE_CLASSES[creature.trim()] ?? creature.trim(), level: Number(level),
        x: Number(coordinates.x), y: Number(coordinates.y), z: Number(coordinates.z), tamed, playerId: tamed ? Number(playerId) : 0 });
      if (current.current === token) setResult(value);
    } catch (reason) { if (current.current === token) setError(describeError(reason)); }
    finally { pending.current = false; if (current.current) setBusy(false); }
  }

  const stalePlayer = sourcePlayerId !== null && (selectedPlayer?.playerId !== sourcePlayerId || selectedPlayer.x === null);
  const disabled = busy || checking || !host;
  const installHelp = [text("首次使用需要在停服时安装生物工具扩展。", "Install the creature extension while the instance is stopped before first use."),
    !stopped ? text("请先停止实例。", "Stop the instance first.") : null,
    text("将从 ArkServerApi 下载已校验版本，仅安装到本实例的独立程序目录；已有第三方加载器冲突时会停止。", "Download a verified ArkServerApi release into this instance's private program directory. An existing conflicting loader stops installation."),
    moduleId === "arksurvivalascended" ? text("安装后，扩展在首次启动或游戏更新时会向 cdn.pelayori.com（备用 cdn.shadowhunter.co.za / cdn.shadowhunter-systems.co.za）发送服务器程序 SHA-256 哈希，下载匹配文件。点击安装即允许此下载。", "On first startup or game updates, the extension sends the server EXE SHA-256 hash to cdn.pelayori.com (fallbacks: cdn.shadowhunter.co.za / cdn.shadowhunter-systems.co.za) for matching files. Installing allows these downloads.") : null].filter(Boolean).join(" ");
  return <section className="ark-creature-spawner" aria-label={text("生成方舟生物", "Spawn ARK creature")}>
    <div className="ark-creature-spawner__status">
      <p role="status">{!host ? text("请在本机桌面客户端使用方舟生物工具。", "Use ARK creature tools in the local desktop client.")
        : checking ? text("正在检查服务端扩展…", "Checking server extension…")
        : extension?.connected ? text("服务端扩展已连接", "Server extension connected")
        : extension?.installed ? running
          ? text("扩展已安装但未连接，请检查运行日志后重试连接。", "Extension installed but disconnected. Check the runtime log and retry the connection.")
          : text("扩展已安装，启动实例后可生成生物。", "Extension installed. Start the instance to spawn creatures.")
        : text("服务端扩展未安装", "Server extension not installed")}</p>
    </div>
    {extension?.issue ? <p className="ark-creature-spawner__note" role="status">{extension.issue}</p> : null}
    <form onSubmit={event => void spawn(event)} onInvalidCapture={event => {
      const node = event.target;
      if (node instanceof HTMLElement) { const details = node.closest("details"); if (details) details.open = true; }
    }}>
      <fieldset disabled={busy}>
        <ArkCatalogPicker kind="creature" query={search} onQueryChange={setSearch} value={creature} onPick={setCreature} />
        <div className="ark-spawn-dock">
        <button type="button" className="secondary-button ark-map-use-player" disabled={!selectedPlayer || selectedPlayer.x === null}
          title={text("将所选玩家的世界坐标和游戏内 PlayerID 填入生成参数。", "Fill spawn parameters with the selected player's world coordinates and in-game PlayerID.")}
          onClick={() => { if (selectedPlayer && selectedPlayer.x !== null && selectedPlayer.y !== null && selectedPlayer.z !== null) {
            setCoordinates({ x: String(selectedPlayer.x), y: String(selectedPlayer.y), z: String(selectedPlayer.z) });
            setPlayerId(String(selectedPlayer.playerId)); setSourcePlayerId(selectedPlayer.playerId);
          } }}>{selectedPlayer ? text(`使用所选玩家 · ${selectedPlayer.name || selectedPlayer.playerId}`, `Use selected player · ${selectedPlayer.name || selectedPlayer.playerId}`)
            : text("选择玩家以填入坐标", "Select a player to fill coordinates")}</button>
        {stalePlayer ? <ActivityNotice tone="warning">{text("所选玩家数据已过期或无角色坐标，请重新选择或手动输入。", "Selected player data is stale or has no coordinates. Select again or enter values manually.")}</ActivityNotice> : null}
        <details className="ark-spawn-options"><summary>{text("坐标与自定义生物", "Coordinates and custom creature")}</summary><div className="ark-creature-spawner__advanced">
        <ConfigurationHelp description={text("当前地图未加载的生物需要完整 /Game/…_C 类路径；Mod 生物需先安装并启用对应 Mod。", "Creatures not loaded by this map need a full /Game/…_C class path. Mod creatures require the corresponding installed and enabled mod.")}>{(help) => <label className="gmt-field" ref={help.anchorRef} {...help.interactionProps}><span>{text("生物类名或完整路径", "Creature class or full path")}</span><input className="gmt-text-input" required maxLength={300} value={creature} onChange={e => setCreature(e.target.value)} /></label>}</ConfigurationHelp>
        <div className="ark-creature-spawner__saved"><label className="gmt-field"><span>{text("收藏名称", "Saved name")}</span><input className="gmt-text-input" value={savedLabel} maxLength={120} onChange={e => setSavedLabel(e.target.value)} /></label>{enabledMods.length ? <label className="gmt-field"><span>Mod</span><select className="gmt-select" value={savedModId} onChange={e => setSavedModId(e.target.value)}><option value="">{text("不关联 Mod", "No mod tag")}</option>{enabledMods.map(id => <option key={id} value={id}>{id}</option>)}</select></label> : null}<button type="button" className="secondary-button" onClick={saveCreature}>{text("收藏生物", "Save creature")}</button></div>
        {custom.length ? <div className="ark-creature-spawner__choices">{custom.map(entry => <button key={entry.classId} type="button" className="secondary-button" onClick={() => setCreature(entry.classId)}>{entry.label || entry.classId}<small>{entry.classId}{entry.modId ? ` · Mod ${entry.modId}` : ""}</small></button>)}</div> : null}
        <ConfigurationHelp description={text("使用游戏世界 X/Y/Z 坐标（厘米），不是地图经纬度。请填写已知安全位置。", "Use game-world X/Y/Z coordinates in centimeters, not map latitude/longitude. Enter a known safe position.")}>{(help) => <div className="ark-creature-spawner__coordinates" ref={help.anchorRef} {...help.interactionProps}>{(["x", "y", "z"] as const).map(axis => <label className="gmt-field" key={axis}><span>{text("世界坐标", "World coordinate")} {axis.toUpperCase()}</span><input className="gmt-text-input" type="number" required min={-10000000} max={10000000} step="any" value={coordinates[axis]} onChange={e => { setSourcePlayerId(null); setCoordinates(previous => ({ ...previous, [axis]: e.target.value })); }} /></label>)}</div>}</ConfigurationHelp>
          {tamed ? <label className="gmt-field"><span>{text("在线玩家的游戏内 ID", "Online in-game Player ID")}</span><input className="gmt-text-input" required type="number" min={1} max={4294967295} step={1} value={playerId} onChange={e => { setSourcePlayerId(null); setPlayerId(e.target.value); }} /></label> : null}
        </div></details>
        <div className="ark-spawn-tool-grid ark-creature-spawner__parameters">
          <label className="ark-spawn-setting"><span>{text("等级", "Level")}</span><input aria-label={text("基础等级", "Base level")} type="number" required min={1} max={5000} step={1} value={level} onChange={e => setLevel(e.target.value)} /></label>
          <label className="ark-spawn-setting"><span>{text("状态", "State")}</span><select aria-label={text("生成状态", "Creature state")} value={String(tamed)} onChange={e => setTamed(e.target.value === "true")}><option value="false">{text("野生", "Wild")}</option><option value="true">{text("驯服", "Tamed")}</option></select></label>
          <ConfigurationActionHelp reason={!host ? text("请在本机桌面客户端使用方舟生物工具。", "Use ARK creature tools in the local desktop client.")
            : !extension?.installed ? installHelp : text("重新检查服务端扩展的连接状态。", "Recheck the server extension connection.")}>
            <button type="button" className="secondary-button ark-spawn-tool" aria-label={extension?.installed ? text("检查连接", "Check connection") : text("安装服务端扩展", "Install server extension")}
              data-ark-tools-action={extension?.installed ? "check" : "install"} disabled={disabled || !extension?.installed && !stopped}
              onClick={() => void (extension?.installed ? refresh() : install())}><ShellIcon name={extension?.installed ? "refresh" : "download"} />
              <span>{busy ? text("处理中", "Working") : extension?.installed ? text("连接", "Connect") : text("安装", "Install")}</span></button>
          </ConfigurationActionHelp>
        <ConfigurationActionHelp reason={!host || busy || checking ? null : !running || !runId ? text("请先启动当前地图。", "Start the selected map first.") : stalePlayer ? text("请重新选择实时玩家或手动输入。", "Select a live player again or enter values manually.") : !configured ? text("请先启用 RCON 并设置管理员密码。", "Enable RCON and configure the admin password first.") : !extension?.connected ? text("服务端扩展未连接。", "Server extension is not connected.") : null}>
        <button type="submit" className="primary-button ark-spawn-tool" aria-label={text("生成生物", "Spawn creature")} disabled={disabled || !running || !runId || stalePlayer || !configured || !extension?.connected}><ShellIcon name={busy ? "loader" : "send"} /><span>{busy ? text("生成中", "Spawning") : text("生成", "Spawn")}</span></button></ConfigurationActionHelp>
        </div></div>
      </fieldset>
    </form>
    <ActivityNotice tone="error">{error ?? ""}</ActivityNotice>
    <ActivityNotice tone="success">{result?.instanceId === instanceId ? `${text("已生成并读回确认", "Spawned and verified")} · ${result.creature.className} · ${text("实际等级", "Actual level")} ${result.creature.level} · ID ${result.creature.id1}:${result.creature.id2} · X ${result.creature.x.toFixed(1)} Y ${result.creature.y.toFixed(1)} Z ${result.creature.z.toFixed(1)}` : ""}</ActivityNotice>
  </section>;
}
