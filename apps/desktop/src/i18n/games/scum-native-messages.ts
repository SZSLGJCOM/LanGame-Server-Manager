import type { MessageCatalog } from "../../i18n-config";
import generalInventory from "../../../../../modules/scum/server-settings-v7/general.json";
import worldInventory from "../../../../../modules/scum/server-settings-v7/world.json";
import featuresInventory from "../../../../../modules/scum/server-settings-v7/features.json";
import respawnInventory from "../../../../../modules/scum/server-settings-v7/respawn.json";
import vehiclesInventory from "../../../../../modules/scum/server-settings-v7/vehicles.json";
import damageInventory from "../../../../../modules/scum/server-settings-v7/damage.json";
import { SCUM_NATIVE_ZH_TITLES } from "./scum-native-zh-titles";

type ScumCopyLocale = "en-US" | "zh-CN";
type ScumCopyPart = "title" | "description";

interface ScumNativeCopySource {
  key: string;
  nativeKey: string;
  title: string;
  type: "boolean" | "integer" | "number" | "string";
  minimum?: number;
  maximum?: number;
  presentation: "specialized" | "generated";
}

const SCUM_NATIVE_COPY_SOURCES = [
  ...generalInventory,
  ...worldInventory,
  ...featuresInventory,
  ...respawnInventory,
  ...vehiclesInventory,
  ...damageInventory
] as ScumNativeCopySource[];

export function scumNativeMessageKey(key: string, part: ScumCopyPart): string {
  return `scum.settings.native.${key}.${part}`;
}

const SCUM_NATIVE_HELP: Readonly<Record<string, readonly [string, string]>> = {
  "logout_timer_in_bunker": ["Seconds before removing the character from the game after the player logs out inside a bunker.", "玩家在地堡内登出后，其角色从游戏中移除前等待的秒数。"],
  "disable_examine_ghost": ["Prevents seasonal holiday ghosts from spawning.", "阻止节日活动中的幽灵生成。"],
  "item_virtualization_visitor_distance_travelled_for_update": ["Distance a player must move before triggering a virtualization update.", "玩家移动达到此距离后，触发一次物品虚拟化更新。"],
  "item_virtualization_visitor_bounds": ["Space around a player considered when restoring virtualized items to the world.", "将虚拟物品恢复到世界中时，检查玩家周边的空间范围。"],
  "virtualized_item_bounds": ["Assumed size of an item when checking whether to restore it to the world.", "判断是否将物品恢复到世界中时，使用的物品估算尺寸。"],
  "item_virtualization_relevancy_update_period": ["Time between virtualization system updates.", "物品虚拟化系统两次更新之间的时间。"],
  "item_virtualization_event_processing_time_budget": ["Maximum processing time allocated to a virtualization system update.", "一次物品虚拟化更新可使用的处理时间。"],
  "kinglet_duster_min_purchased_amount": ["Minimum number of Kinglet Duster vehicles available to buy.", "可供购买的Kinglet Duster 的最低数量。"],
  "dirtbike_min_purchased_amount": ["Minimum number of Dirtbike vehicles available to buy.", "可供购买的越野摩托的最低数量。"],
  "laika_min_purchased_amount": ["Minimum number of Laika vehicles or their engines available to buy.", "可供购买的Laika 或其引擎的最低数量。"],
  "motorboat_min_purchased_amount": ["Minimum number of Motorboat vehicles available to buy.", "可供购买的摩托艇的最低数量。"],
  "wheelbarrow_min_purchased_amount": ["Minimum number of Wheelbarrow vehicles available to buy.", "可供购买的手推车的最低数量。"],
  "wolfswagen_min_purchased_amount": ["Minimum number of Wolfswagen vehicles or their engines available to buy.", "可供购买的Wolfswagen 或其引擎的最低数量。"],
  "bicycle_min_purchased_amount": ["Minimum number of Bicycle vehicles available to buy.", "可供购买的自行车的最低数量。"],
  "rager_min_purchased_amount": ["Minimum number of Rager vehicles or their engines available to buy.", "可供购买的Rager 或其引擎的最低数量。"],
  "cruiser_min_purchased_amount": ["Minimum number of Cruiser vehicles or their engines available to buy.", "可供购买的Cruiser 或其引擎的最低数量。"],
  "ris_min_purchased_amount": ["Minimum number of Ris vehicles available to buy.", "可供购买的RIS 的最低数量。"],
  "dinghy_min_purchased_amount": ["Minimum number of Dinghy vehicles or their engines available to buy.", "可供购买的橡皮艇或其引擎的最低数量。"],
  "sup_min_purchased_amount": ["Minimum number of SUP vehicles available to buy.", "可供购买的SUP 桨板的最低数量。"],
  "kinglet_mariner_min_purchased_amount": ["Minimum number of Kinglet Mariner vehicles available to buy.", "可供购买的Kinglet Mariner 的最低数量。"],
  "tractor_min_purchased_amount": ["Minimum number of Tractor vehicles or their engines available to buy.", "可供购买的拖拉机或其引擎的最低数量。"],
  "sidecar_bike_min_purchased_amount": ["Minimum number of Sidecar Bike vehicles or their engines available to buy.", "可供购买的边车摩托车或其引擎的最低数量。"],
  "time_of_day_speed": ["Relative to real time: 1 makes a full day last 24 hours; higher values advance time faster.", "相对于现实时间的倍率：1 表示游戏一天持续 24 小时；数值越高，时间流逝越快。"],
  "puppet_health_multiplier": ["Applies to every puppet type, including suicide puppets; their relative base health differences remain.", "同时影响各类傀儡，包括自爆傀儡；各类傀儡原有的基础生命值差异仍然保留。"],
  "raid_protection_type": ["3 selects global raid protection. An empty raid schedule provides no protection.", "3 表示全局袭击保护；袭击时段表为空时，基地不会受到保护。"],
  "raid_protection_global_should_show_raid_times_message": ["Includes the raid schedule in the welcome message and message of the day.", "在欢迎消息和每日消息中显示袭击时段。"],
  "raid_protection_global_should_show_raid_announcement_message": ["Announces upcoming raid starts and ends, using the advance times in the raid schedule.", "按照袭击时段表设置的提前量，预告袭击开始和结束。"],
  "raid_protection_global_should_show_raid_start_end_messages": ["Announces the actual start and end of each raid period.", "在袭击时段实际开始和结束时发送公告。"],
  "log_vehicle_destroyed": ["Records destruction, inactivity expiry, and disappearance, including sales to traders.", "记录载具损毁、闲置过期和消失事件；出售给商人也会产生消失记录。"],
  "abandoned_bunker_reset_armory_lockers_on_activation_only": ["Restocks armory loot only when the bunker activates naturally or with a keycard.", "仅在地堡自然激活或使用门卡激活时补充军械柜战利品。"]
};

const SCUM_JSON_HELP: Readonly<Record<string, readonly [string, string]>> = {
  "economy-reset-time-hours": ["Hours until trader funds and stock reset; a negative value disables resets.", "商人资金和库存重置的间隔，单位为小时；负数禁用重置。"],
  "prices-randomization-time-hours": ["Hours between price randomizations; a negative value disables them.", "价格重新随机化的间隔，单位为小时；负数禁用随机化。"],
  "fully-restock-tradeable-hours": ["Hours needed for a full restock; a negative value disables restocking.", "库存完全补满所需的小时数；负数禁用补货。"],
  "traders-unlimited-funds": ["Allows traders to keep paying when players sell items, without exhausting their funds.", "玩家出售物品时，商人的可用资金不会耗尽。"],
  "tradeable-code": ["Asset identifier used by the item or vehicle spawn command.", "物品或载具生成命令使用的资产标识。"],
  "base-purchase-price": ["-1 retains the default base price, before price adjustments.", "-1 保留默认基础价格；该价格尚未应用浮动调整。"],
  "base-sell-price": ["-1 retains the default base price, before price adjustments.", "-1 保留默认基础价格；该价格尚未应用浮动调整。"],
  "delta-price": ["-1 uses a random price multiplier; 0 or higher fixes it across price randomizations.", "-1 使用随机价格倍率；0 或正数固定此倍率，不再随价格随机化改变。"],
  "can-be-purchased": ["default preserves native availability; true allows purchasing; false prevents purchasing.", "default 沿用原生供应规则；true 允许购买；false 禁止购买。"],
  "day": ["Accepts weekday names, ranges, or groups such as Weekend.", "可填写星期名称、星期范围或 Weekend 等星期分组。"],
  "time": ["Uses server-local time. Raid ranges allow raiding; notification ranges repeat messages.", "使用服务器本地时间。袭击时间段表示允许袭击；通知时间段表示循环显示消息。"],
  "start-announcement-time": ["Minutes before raiding starts; 0 disables the advance announcement.", "在袭击开始前多少分钟预告；0 禁用提前公告。"],
  "end-announcement-time": ["Minutes before raiding ends; 0 disables the advance announcement.", "在袭击结束前多少分钟预告；0 禁用提前公告。"],
  "duration": ["How many seconds the notification remains visible.", "每次通知在屏幕上显示的秒数。"],
  "color": ["RGB components separated by hyphens, for example 255-255-255.", "用连字符分隔 RGB 分量，例如 255-255-255。"],
  "wait": ["Minutes between repeated notifications within a time window.", "在通知时间段内，两次显示之间的分钟数。"],
  "message": ["Supports #NumPlayers, #Date, #Time, #RestartIn(HH:MM), and #RestartAt(HH:MM).", "支持 #NumPlayers、#Date、#Time、#RestartIn(HH:MM) 和 #RestartAt(HH:MM) 占位符。"]
};

function resolveChineseTitle(setting: ScumNativeCopySource): string {
  const title = SCUM_NATIVE_ZH_TITLES[setting.key];
  if (!title) {
    throw new Error(`Missing zh-CN title for SCUM native setting: ${setting.key}`);
  }
  return title;
}

export function buildScumNativeMessageCatalog(locale: ScumCopyLocale): MessageCatalog {
  const messages: MessageCatalog = {};
  for (const setting of SCUM_NATIVE_COPY_SOURCES) {
    if (setting.presentation !== "specialized") continue;
    const title = locale === "zh-CN" ? resolveChineseTitle(setting) : setting.title;
    messages[scumNativeMessageKey(setting.key, "title")] = title;
    const help = SCUM_NATIVE_HELP[setting.key];
    if (help) messages[scumNativeMessageKey(setting.key, "description")] = help[locale === "zh-CN" ? 1 : 0];
  }
  return messages;
}

const SCUM_JSON_FIELDS = [
  "economy-reset-time-hours", "prices-randomization-time-hours", "tradeable-rotation-time-ingame-hours-min",
  "tradeable-rotation-time-ingame-hours-max", "tradeable-rotation-time-of-day-min",
  "tradeable-rotation-time-of-day-max", "fully-restock-tradeable-hours",
  "trader-funds-change-rate-per-hour-multiplier", "prices-subject-to-delta", "prices-subject-to-player-count",
  "gold-price-subject-to-global-multiplier", "gold-base-price", "gold-sale-price-modifier",
  "gold-price-change-percentage-step", "gold-price-change-per-step", "economy-logging",
  "traders-unlimited-funds", "traders-unlimited-stock",
  "global-only-after-player-sale-tradeable-availability-enabled", "tradeable-rotation-enabled",
  "enable-fame-point-requirement", "trader", "tradeable-code", "base-purchase-price", "base-sell-price",
  "delta-price", "can-be-purchased", "required-famepoints", "available-after-sale-only", "day", "time",
  "start-announcement-time", "end-announcement-time", "duration", "color", "wait", "message"
] as const;

const SCUM_JSON_ZH_TITLES: Readonly<Record<string, string>> = {
  "economy-reset-time-hours": "经济重置间隔（小时）",
  "prices-randomization-time-hours": "价格随机化间隔（小时）",
  "tradeable-rotation-time-ingame-hours-min": "交易品轮换最短游戏时长",
  "tradeable-rotation-time-ingame-hours-max": "交易品轮换最长游戏时长",
  "tradeable-rotation-time-of-day-min": "交易品轮换最早时刻",
  "tradeable-rotation-time-of-day-max": "交易品轮换最晚时刻",
  "fully-restock-tradeable-hours": "交易品完全补货时长",
  "trader-funds-change-rate-per-hour-multiplier": "商人资金每小时变化倍率",
  "prices-subject-to-delta": "价格受差价影响",
  "prices-subject-to-player-count": "价格受玩家数量影响",
  "gold-price-subject-to-global-multiplier": "黄金价格受全局倍率影响",
  "gold-base-price": "黄金基础价格", "gold-sale-price-modifier": "黄金售价修正值",
  "gold-price-change-percentage-step": "黄金价格百分比变化步长",
  "gold-price-change-per-step": "黄金价格单步变化量", "economy-logging": "经济日志",
  "traders-unlimited-funds": "商人无限资金", "traders-unlimited-stock": "商人无限库存",
  "global-only-after-player-sale-tradeable-availability-enabled": "交易品仅在玩家出售后全局供应",
  "tradeable-rotation-enabled": "启用交易品轮换", "enable-fame-point-requirement": "启用声望点数要求",
  trader: "商人", "tradeable-code": "交易品代码", "base-purchase-price": "基础买入价",
  "base-sell-price": "基础卖出价", "delta-price": "浮动差价", "can-be-purchased": "允许购买",
  "required-famepoints": "所需声望点数", "available-after-sale-only": "仅在出售后供应",
  day: "星期规则", time: "时间", "start-announcement-time": "开始公告提前量（分钟）",
  "end-announcement-time": "结束公告提前量（分钟）", duration: "显示时长（秒）", color: "RGB 颜色",
  wait: "重复间隔（分钟）", message: "消息"
};

const SCUM_JSON_EN_TITLES: Readonly<Record<string, string>> = {
  day: "Weekday Rule", duration: "Display Duration (Seconds)", wait: "Repeat Interval (Minutes)",
  "start-announcement-time": "Start Announcement Lead Time (Minutes)",
  "end-announcement-time": "End Announcement Lead Time (Minutes)", color: "RGB Color"
};

function humanizeJsonKey(key: string): string {
  return key.split("-").map((word) => word ? word[0].toUpperCase() + word.slice(1) : word).join(" ");
}

export function scumJsonMessageKey(key: string, part: ScumCopyPart): string {
  return `scum.settings.json.fields.${key}.${part}`;
}

export function buildScumJsonMessageCatalog(locale: ScumCopyLocale): MessageCatalog {
  const messages: MessageCatalog = {};
  for (const key of SCUM_JSON_FIELDS) {
    const title = locale === "zh-CN" ? SCUM_JSON_ZH_TITLES[key] : SCUM_JSON_EN_TITLES[key] ?? humanizeJsonKey(key);
    messages[scumJsonMessageKey(key, "title")] = title;
    const help = SCUM_JSON_HELP[key];
    if (help) messages[scumJsonMessageKey(key, "description")] = help[locale === "zh-CN" ? 1 : 0];
  }
  return messages;
}
