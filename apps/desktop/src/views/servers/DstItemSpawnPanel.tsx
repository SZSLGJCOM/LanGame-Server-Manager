import { useMemo, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { DontStarveShard } from "../settings/modules/dontstarve-shards";
import { DstPrefabCategories, DstPrefabPicker } from "./DstPrefabPicker";
import { getGmToolCatalog, getInitialGmToolValues } from "./gm-tools";
import type { DstGmCommand } from "./use-dst-gm-command";

const TOOL_ID = "dst_give_item_to_player";
const AMOUNT_MIN = 1, AMOUNT_MAX = 999;

export type DstItemSpawn = ReturnType<typeof useDstItemSpawn>;

/** One item-spawn form shared by the rail's picker (top) and its pinned GM grid (bottom). */
export function useDstItemSpawn() {
  const defaults = useMemo(() => {
    const tool = getGmToolCatalog("dontstarve")?.tools.find(item => item.id === TOOL_ID);
    return tool ? getInitialGmToolValues(tool) : {};
  }, []);
  const [values, setValues] = useState(defaults);
  const [query, setQuery] = useState("");
  const update = (key: string, value: string) => setValues(current => ({ ...current, [key]: value }));
  return { values, query, setQuery, update };
}

/** The flexible top area: search box and the paged item results. */
export function DstItemSpawnPicker({ spawn }: { spawn: DstItemSpawn }) {
  const { locale, t } = useI18n();
  return <section className="dst-gm-spawn" aria-label={selectLocaleText(locale, "生成物品", "Spawn items")}>
    <DstPrefabPicker locale={locale} query={spawn.query} onQueryChange={spawn.setQuery} onPick={prefab => spawn.update("prefab", prefab)} t={t} value={spawn.values.prefab ?? ""} />
  </section>;
}

interface ToolsProps { spawn: DstItemSpawn; command: DstGmCommand; shard: DontStarveShard; target: { id: string; name: string } | null }

/** The pinned grid: category toggles, then 所有人 | 进背包 | 数量 | 发放 (labels fit 46px cells; tooltips carry 所有玩家/放入背包), all GM tool cells in four columns. */
export function DstItemSpawnTools({ spawn, command, shard, target }: ToolsProps) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const { values, update } = spawn;
  const allPlayers = values.allPlayers === "true", inventory = values.placeInInventory !== "false";
  const amount = Number(values.amount);
  const amountValid = /^\d+$/.test(values.amount ?? "") && amount >= AMOUNT_MIN && amount <= AMOUNT_MAX;
  const sending = command.sendingToolId === TOOL_ID;
  const reason = !allPlayers && !target ? text("先在地图或玩家列表中选择玩家，或打开所有玩家", "Select a player on the map or in the list, or choose all players")
    : !values.prefab ? text("先在上方选择物品", "Pick an item above first")
    : !amountValid ? text(`数量须为 ${AMOUNT_MIN}–${AMOUNT_MAX} 的整数`, `Amount must be a whole number from ${AMOUNT_MIN} to ${AMOUNT_MAX}`) : null;
  const targetLabel = allPlayers ? text("本分片所有玩家", "everyone on this shard") : target?.name;
  const toggle = (key: string, on: boolean, icon: "users" | "package", label: string, title: string) =>
    <button type="button" className="secondary-button dst-gm-tool" aria-pressed={on} title={title} onClick={() => update(key, String(!on))}>
      <ShellIcon name={icon} /><span>{label}</span></button>;
  return <section className="dst-gm-tools" aria-label={text("发放物品", "Give items")}>
    <DstPrefabCategories locale={locale} query={spawn.query} onQueryChange={spawn.setQuery} t={t} />
    <div className="dst-gm-tool-grid">
      {toggle("allPlayers", allPlayers, "users", text("所有人", "All"), text("所有玩家：发放给本分片所有玩家", "All players: give to everyone on this shard"))}
      {toggle("placeInInventory", inventory, "package", text("进背包", "Inventory"), inventory ? text("放入背包（关闭则生成在脚下）", "Into the inventory (off: spawn at the feet)") : text("生成在玩家脚下", "Spawn at the player's feet"))}
      <label className={`dst-gm-amount${amountValid ? "" : " is-invalid"}`} title={text(`数量（${AMOUNT_MIN}–${AMOUNT_MAX}）`, `Amount (${AMOUNT_MIN}–${AMOUNT_MAX})`)}>
        <span>{text("数量", "Amount")}</span>
        <input type="number" inputMode="numeric" min={AMOUNT_MIN} max={AMOUNT_MAX} step={1} value={values.amount ?? ""} aria-invalid={!amountValid}
          aria-label={text("数量", "Amount")} onChange={event => update("amount", event.target.value)} />
      </label>
      <button type="button" className="primary-button dst-gm-tool dst-gm-send" disabled={command.busy || Boolean(reason)}
        title={reason ?? text(`发放给 ${targetLabel}`, `Give to ${targetLabel}`)}
        onClick={() => void command.run(TOOL_ID, { ...values, shard, playerId: target?.id ?? "" })}>
        <ShellIcon name={sending ? "loader" : "send"} /><span>{sending ? text("发送中", "Sending") : text("发放", "Give")}</span>
      </button>
    </div>
  </section>;
}
