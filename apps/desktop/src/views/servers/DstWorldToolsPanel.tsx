import type { ReactNode } from "react";
import { ShellIcon, type ShellIconName } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { DontStarveShard } from "../settings/modules/dontstarve-shards";
import type { DstGmCommand } from "./use-dst-gm-command";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";

const SEASONS: { value: string; zh: string; en: string; icon: ShellIconName }[] = [
  { value: "spring", zh: "春季", en: "Spring", icon: "star" },
  { value: "summer", zh: "夏季", en: "Summer", icon: "sun" },
  { value: "autumn", zh: "秋季", en: "Autumn", icon: "tag" },
  { value: "winter", zh: "冬季", en: "Winter", icon: "moon" }
];
const PHASES: { value: string; zh: string; en: string; icon: ShellIconName; title?: [string, string] }[] = [
  { value: "day", zh: "白天", en: "Day", icon: "sun" },
  { value: "dusk", zh: "黄昏", en: "Dusk", icon: "clock" },
  { value: "night", zh: "夜晚", en: "Night", icon: "moon" },
  { value: "next", zh: "下一天", en: "Next day", icon: "chevron-right", title: ["跳过一天，直接到下一天清晨", "Skip to the next morning"] }
];

export type DstMapPick = "player" | "point";

interface Props {
  command: DstGmCommand; season: string | null; phase: string | null; shard: DontStarveShard; target: { id: string; name: string } | null;
  /** Why Master-driven world actions and actions on the viewed shard cannot run now. */
  reasons: { world: string | null; player: string | null; point: string | null };
  /** The confirmed world regeneration button, the last world tool. */
  regenerate?: ReactNode;
  /** A teleport waiting for its destination: the next player chip or map click supplies it. */
  pick: DstMapPick | null;
  onPick: (pick: DstMapPick | null) => void;
}

/** World and player GM actions as icon buttons in two four-column groups; each sends immediately through the shared DST channel. */
export function DstWorldToolsPanel({ command, season, phase, shard, target, reasons, regenerate, pick, onPick }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const busy = command.busy;
  const tool = (key: string, icon: ShellIconName, label: string, run: () => void, options: { pressed?: boolean; reason?: string | null; title?: string } = {}) => {
    const reason = options.reason === undefined ? reasons.world : options.reason;
    return <button key={key} type="button" className="secondary-button dst-gm-tool" disabled={busy || Boolean(reason)} aria-pressed={options.pressed}
      title={reason ?? options.title ?? label} onClick={run}><ShellIcon name={icon} /><span>{label}</span></button>;
  };
  const playerReason = reasons.player ?? (target ? null : text("先在地图或玩家列表中选择玩家", "Select a player on the map or in the list first"));
  const player = (key: string, icon: ShellIconName, label: string, tool_id: string, zh: string, en: string) =>
    tool(key, icon, label, () => void command.run(tool_id, { shard, playerId: target?.id ?? "" }),
      { reason: playerReason, title: target ? text(`${zh}：${target.name}`, `${en}: ${target.name}`) : undefined });
  const picker = (mode: DstMapPick, icon: ShellIconName, label: string, zh: string, en: string, extra: string | null) =>
    tool(mode, icon, label, () => onPick(pick === mode ? null : mode),
      { pressed: pick === mode, reason: playerReason ?? extra, title: pick === mode ? text("再次点击取消", "Click again to cancel") : text(zh, en) });
  const clearReason = reasons.player;
  return <section className="dst-gm-tools" aria-label={text("GM 工具", "GM tools")}>
    <section className="dst-gm-tool-group" aria-label={text("世界", "World")}>
      <h3>{text("世界", "World")}</h3>
      <div className="dst-gm-tool-grid">
        {PHASES.map(item => tool(item.value, item.icon, text(item.zh, item.en), () => void command.run("dst_set_phase", { phase: item.value }),
          { pressed: item.value === "next" ? undefined : phase === item.value, title: item.title && text(...item.title) }))}
        {SEASONS.map(item => tool(item.value, item.icon, text(item.zh, item.en),
          () => void command.run("dst_set_season", { season: item.value }), { pressed: season === item.value }))}
        {tool("rain", "play", text("下雨", "Rain"), () => void command.run("dst_set_rain", { enabled: "true" }))}
        {tool("dry", "stop-circle", text("停雨", "Stop rain"), () => void command.run("dst_set_rain", { enabled: "false" }))}
        <InlineConfirmAction className="secondary-button dst-gm-tool" wrapperClassName="dst-gm-confirm" scopeKey={`dst-clear:${shard}`} disabled={busy || Boolean(clearReason)}
          title={clearReason ?? text(`清理分片 ${shard} 地面上的掉落物`, `Clear dropped items on shard ${shard}`)}
          confirmation={text(`删除分片 ${shard} 地面上所有无主掉落物？不可撤销；不可替代物品、重物、装备、容器、陷阱和生物会保留。`,
            `Delete every unowned item lying on shard ${shard}? This cannot be undone; irreplaceable, heavy, equippable, container, trap and living items stay.`)}
          confirmLabel={text("清理", "Clear")} onConfirm={() => command.run("dst_clear_ground_items", { shard })}>
          <ShellIcon name="trash" /><span>{text("清理", "Clear")}</span>
        </InlineConfirmAction>
        {regenerate}
      </div>
    </section>
    <section className="dst-gm-tool-group" aria-label={text("玩家", "Player")}>
      <h3>{text("玩家", "Player")}{target ? <span>{target.name}</span> : null}</h3>
      <div className="dst-gm-tool-grid">
        {player("revive", "user", text("复活", "Revive"), "dst_revive_player", "复活", "Revive")}
        {player("restore", "check-circle", text("回满", "Restore"), "dst_restore_player", "生命、饥饿、理智回满并擦干、恢复体温", "Fill health, hunger and sanity, dry off and reset temperature")}
        {player("god", "shield", text("无敌", "God"), "dst_toggle_godmode", "切换无敌（状态以服务器回执为准）", "Toggle invincibility (see the server reply for the state)")}
        {picker("player", "users", text("到玩家", "To player"), "点击后再选择同分片的目标玩家", "Then pick a destination player on this shard", null)}
        {picker("point", "circle-dot", text("到地点", "To point"), "点击后在地图上点选目的地", "Then click a destination on the map", reasons.point)}
        {player("unlock", "lock", text("解锁", "Unlock"), "dst_unlock_recipes", "解锁全部科技（自由制作）", "Unlock every recipe (free crafting)")}
      </div>
    </section>
  </section>;
}
