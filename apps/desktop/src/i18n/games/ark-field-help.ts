import type { MessageCatalog } from "../../i18n-config";

const HELP: Record<string, readonly [string, string]> = {
  cluster_id: [
    "Maps in one cluster must use the same ID and shared cluster directory. Setting only the ID keeps each instance's uploads separate.",
    "同一集群的地图须使用相同 ID 和共享集群目录。只填写相同 ID 时，各实例的上传数据仍分别保存。"
  ],
  cluster_directory: [
    "Absolute directory shared by maps in this cluster, including a local drive or UNC share. Leave empty to keep this instance's local cluster directory. Existing uploads are not moved when the directory changes.",
    "填写同一集群各地图共同使用的绝对目录，可使用本地磁盘或 UNC 共享路径。留空继续使用本实例的集群目录；修改目录不会搬移已有上传数据。"
  ],
  no_transfer_from_filtering: [
    "Block ARK Data transfers from single-player worlds and servers outside this cluster.",
    "阻止从单人世界及当前集群以外的服务器传入 ARK 数据。"
  ],
  mating_interval_multiplier: [
    "Scales the wait between mating attempts. Higher values mean a longer wait; lower values allow more frequent mating.",
    "调整两次交配之间的等待时间。数值越大，等待越久；数值越小，可更频繁交配。"
  ],
  mating_speed_multiplier: [
    "Scales mating progress speed. Higher values complete mating faster.",
    "调整交配进度速度。数值越大，完成交配所需时间越短。"
  ],
  egg_hatch_speed_multiplier: [
    "Scales egg incubation and pregnancy speed. Higher values shorten the time to hatch or give birth.",
    "调整蛋孵化及妊娠速度。数值越大，孵化或分娩所需时间越短。"
  ],
  baby_mature_speed_multiplier: [
    "Scales offspring maturation speed. Higher values shorten maturation and leave less time for imprint care.",
    "调整幼崽成长速度。数值越大，成年越快，可进行留痕照料的总时间也越短。"
  ],
  baby_cuddle_interval_multiplier: [
    "Scales the time between imprint care requests. Lower values make care requests more frequent; coordinate this with maturation speed.",
    "调整两次留痕照料请求的间隔。数值越小，请求越频繁；应与幼崽成长速度共同设置。"
  ],
  baby_cuddle_grace_period_multiplier: [
    "Scales the grace period after a care request. Higher values allow more time before missed care reduces imprint quality.",
    "调整照料请求出现后的宽限时间。数值越大，逾期照料导致留痕衰减前的等待越长。"
  ],
  baby_cuddle_lose_imprint_quality_speed_multiplier: [
    "Scales how quickly imprint quality falls after the care grace period expires. Higher values cause faster loss.",
    "调整照料宽限期结束后留痕品质的下降速度。数值越大，留痕衰减越快。"
  ],
  baby_imprinting_stat_scale_multiplier: [
    "Scales the attribute bonus provided by imprinting; it does not set the progress earned by a care action.",
    "调整留痕带来的属性加成强度，不控制一次照料增加的留痕进度。"
  ],
  baby_imprint_amount_multiplier: [
    "Scales imprint progress earned per completed care request. Higher values increase progress per action.",
    "调整每次完成照料所增加的留痕进度。数值越大，单次获得的留痕进度越多。"
  ],
  allow_anyone_baby_imprint_cuddle: [
    "Allow a player other than the original imprint owner to perform baby care.",
    "允许原留痕主人之外的玩家照料幼崽。"
  ],
  disable_imprint_dino_buff: [
    "Disable the combat bonus received when an imprinted creature is ridden by its imprint owner.",
    "禁用留痕主人骑乘该生物时获得的战斗增益。"
  ],
  usestore: [
    "Store character data with the world save instead of separate persistent .arkprofile files. This selects ARK's native stored-data format, not an in-game shop.",
    "将角色数据整合到世界存档，不再持续保存独立 .arkprofile 文件。此项选择 ARK 原生角色存储格式，不是游戏商店开关。"
  ]
};

export function buildArkFieldHelpMessages(
  moduleId: "arksurvivalascended" | "arksurvivalevolved",
  locale: "en-US" | "zh-CN"
): MessageCatalog {
  const index = locale === "zh-CN" ? 1 : 0;
  const prefix = `settings.schema.${moduleId}`;
  const catalog: MessageCatalog = Object.fromEntries(Object.entries(HELP).map(([key, descriptions]) => [
    `${prefix}.${key}.description`, descriptions[index]
  ]));
  catalog[`${prefix}.cluster_directory.title`] = index === 1 ? "共享集群目录" : "Shared Cluster Directory";
  if (moduleId === "arksurvivalevolved") {
    catalog[`${prefix}.imprintlimit.description`] = index === 1
      ? "自动销毁留痕比例超过此百分比阈值的生物。此项是异常生物清理阈值，不是留痕增长倍率。"
      : "Automatically destroy creatures whose imprint percentage exceeds this threshold. This is a cleanup threshold, not an imprint gain multiplier.";
  } else {
    catalog[`${prefix}.enable_steel_shield.description`] = index === 1
      ? "启用 Nitrado SteelShield 抗 DDoS 服务；仅适用于由 Nitrado 托管的服务器。"
      : "Enable Nitrado SteelShield anti-DDoS protection. This option only applies to servers hosted by Nitrado.";
  }
  return catalog;
}
