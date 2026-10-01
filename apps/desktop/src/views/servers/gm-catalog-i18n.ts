import { isChineseLocale, selectLocaleText } from "../../i18n-config";

type CatalogLocale = string | null | undefined;

interface ArkCatalogLike {
  name: string;
  category: string;
}

interface DstPrefabLike {
  value: string;
  label: string;
  category: string;
}

const ARK_CATEGORY_ZH: Record<string, string> = {
  Ammunition: "弹药",
  Armor: "护甲",
  Artifact: "神器",
  Attachment: "配件",
  "Chibi Pet": "迷你宠物",
  Consumable: "消耗品",
  Dye: "染料",
  Egg: "蛋",
  Emote: "表情",
  Fertilizer: "肥料",
  Flag: "旗帜",
  Hairstyle: "发型",
  Item: "物品",
  Official: "官方",
  Recipe: "配方",
  Resource: "资源",
  Saddle: "鞍具",
  Seed: "种子",
  Skin: "皮肤",
  Structure: "建筑",
  Tool: "工具",
  Tribute: "贡品",
  Trophy: "战利品",
  Vehicle: "载具",
  Weapon: "武器"
};

const ARK_ITEM_NAME_ZH: Record<string, string> = {
  "Advanced Bullet": "高级子弹",
  "Advanced Rifle Bullet": "高级步枪子弹",
  "Angler Gel": "安康鱼油",
  "Black Pearl": "黑珍珠",
  "Cementing Paste": "水泥",
  Charcoal: "木炭",
  Chitin: "甲壳素",
  "Chitin or Keratin": "甲壳素或角质",
  Crystal: "水晶",
  Electronics: "电子元件",
  Element: "能量元素",
  "Element Dust": "元素粉尘",
  "Element Shard": "元素碎片",
  Fiber: "纤维",
  Flint: "燧石",
  Gasoline: "汽油",
  Gunpowder: "火药",
  Hide: "兽皮",
  Keratin: "角质",
  Metal: "金属",
  "Metal Ingot": "金属锭",
  Narcotic: "麻醉药",
  Obsidian: "黑曜石",
  Oil: "油",
  "Organic Polymer": "有机聚合物",
  Pelt: "毛皮",
  Polymer: "聚合物",
  "Raw Meat": "生肉",
  "Raw Prime Meat": "优质生肉",
  "Rare Flower": "稀有花朵",
  "Rare Mushroom": "稀有蘑菇",
  "Simple Bullet": "简易子弹",
  "Simple Pistol": "简易手枪",
  "Sparkpowder": "引火粉",
  Stone: "石头",
  "Stone Arrow": "石箭",
  "Stone Hatchet": "石斧",
  "Stone Pick": "石镐",
  Thatch: "茅草",
  "Tranq Arrow": "麻醉箭",
  "Water Jar": "水瓶",
  Wood: "木头"
};

const ARK_ITEM_TERM_ZH: Record<string, string> = {
  Advanced: "高级",
  Arrow: "箭",
  Behemoth: "巨型",
  Blueprint: "蓝图",
  Boots: "靴子",
  Bow: "弓",
  Bullet: "子弹",
  Ceiling: "天花板",
  Cliff: "悬崖",
  Chitin: "甲壳",
  Cloth: "布制",
  Dinosaur: "恐龙",
  Door: "门",
  Doorframe: "门框",
  Double: "双开",
  Fence: "栅栏",
  Flexible: "柔性",
  Fireplace: "壁炉",
  Foundation: "地基",
  Gate: "大门",
  Gateframe: "大门框",
  Gateway: "门框",
  Giant: "巨型",
  Gloves: "手套",
  Hatchframe: "舱门框",
  Helmet: "头盔",
  Hide: "兽皮",
  Inclined: "倾斜",
  Intake: "进水口",
  Intersection: "交叉",
  Irrigation: "灌溉",
  Ladder: "梯子",
  Large: "大型",
  Left: "左",
  Medium: "中型",
  Metal: "金属",
  Pick: "镐",
  Pillar: "柱子",
  Pipe: "管道",
  Platform: "平台",
  Railing: "栏杆",
  Ramp: "斜坡",
  Right: "右",
  Roof: "屋顶",
  Saddle: "鞍",
  Shield: "盾",
  Shirt: "上衣",
  Simple: "简易",
  Sloped: "斜坡",
  Small: "小型",
  Spike: "尖刺",
  Staircase: "楼梯",
  Stairs: "楼梯",
  Stone: "石头",
  Straight: "直线",
  Storage: "储物",
  Support: "支柱",
  Sword: "剑",
  Tap: "水龙头",
  Thatch: "茅草",
  Triangle: "三角",
  Trapdoor: "活板门",
  Vertical: "垂直",
  Wall: "墙",
  Windowframe: "窗框",
  Window: "窗户",
  Wood: "木质"
};

const ARK_CREATURE_NAME_ZH: Record<string, string> = {
  Achatina: "玛瑙螺",
  Allosaurus: "异特龙",
  Ankylosaurus: "甲龙",
  Anglerfish: "安康鱼",
  Araneo: "蜘蛛",
  Arthropluera: "古马陆",
  Baryonyx: "重爪龙",
  Basilisk: "毒蜥",
  Beelzebufo: "魔鬼蛙",
  Brontosaurus: "雷龙",
  Carbonemys: "淡水碳龟",
  Carnotaurus: "牛龙",
  Cnidaria: "水母",
  Coelacanth: "腔棘鱼",
  Dilophosaur: "双脊龙",
  Dimetrodon: "异齿龙",
  Dimorphodon: "双型齿翼龙",
  Diplocaulus: "笠头螈",
  Diplodocus: "梁龙",
  Direbear: "恐熊",
  Direwolf: "恐狼",
  Dodo: "渡渡鸟",
  Doedicurus: "星尾兽",
  Dragon: "喷火龙",
  Equus: "庞马",
  Giganotosaurus: "南方巨兽龙",
  Gigantopithecus: "巨猿",
  Iguanodon: "禽龙",
  Kairuku: "伪齿鸟",
  Kaprosuchus: "猪鳄",
  Lystrosaurus: "水龙兽",
  Mammoth: "猛犸象",
  Manta: "蝠鲼",
  Megalania: "古巨蜥",
  Megaloceros: "大角鹿",
  Megalodon: "巨齿鲨",
  Megalosaurus: "斑龙",
  Meganeura: "巨脉蜻蜓",
  Mosasaurus: "沧龙",
  Moschops: "麝足兽",
  Otter: "水獭",
  Oviraptor: "窃蛋龙",
  Ovis: "绵羊",
  Pachy: "肿头龙",
  Paraceratherium: "巨犀",
  Parasaur: "副栉龙",
  Piranha: "食人鱼",
  Procoptodon: "巨型袋鼠",
  Pteranodon: "无齿翼龙",
  Pulmonoscorpius: "巨蝎",
  Purlovia: "兽头兽",
  Raptor: "迅猛龙",
  Rex: "霸王龙",
  Sarco: "帝鳄",
  Spino: "棘背龙",
  Stegosaurus: "剑龙",
  Therizinosaur: "镰刀龙",
  Titanoboa: "泰坦巨蟒",
  Triceratops: "三角龙",
  Trilobite: "三叶虫",
  Tusoteuthis: "托斯特巨鱿",
  Wyvern: "飞龙",
  Yutyrannus: "羽暴龙"
};

const ARK_CREATURE_PREFIX_ZH: Record<string, string> = {
  Aberrant: "畸变",
  Abyssal: "深渊",
  Alpha: "精英",
  Brute: "凶暴",
  Corrupted: "腐化",
  Eerie: "诡异",
  Enraged: "狂怒",
  Malfunctioned: "故障",
  Skeletal: "骨架",
  Tek: "泰克",
  Young: "幼年",
  Zombie: "僵尸"
};

const ARK_PARENTHESES_ZH: Record<string, string> = {
  Center: "中心岛",
  "The Center": "中心岛",
  Gauntlet: "挑战",
  Hunt: "狩猎",
  Rift: "裂隙",
  Retrieve: "回收"
};

const ARK_COMMON_TERM_ZH: Record<string, string> = {
  Absorbent: "吸附",
  Acid: "酸液",
  Air: "空气",
  Ambergris: "龙涎香",
  Ammonite: "菊石",
  Anniversary: "周年",
  Attack: "攻击",
  Ball: "球",
  Battery: "电池",
  Beetle: "甲虫",
  Bile: "胆汁",
  Bio: "生物",
  Blood: "血",
  Blue: "蓝色",
  Bone: "骨",
  Boss: "首领",
  Broodmother: "育母蛛",
  Cake: "蛋糕",
  Candle: "蜡烛",
  Carno: "牛龙",
  Charge: "充能",
  Clay: "黏土",
  Coal: "煤",
  Condensed: "凝缩",
  Congealed: "凝结",
  Corrupted: "腐化",
  Crafted: "制作",
  Crystal: "水晶",
  Crystalized: "结晶",
  Deathworm: "死亡蠕虫",
  Defense: "防御",
  Dermis: "真皮",
  Drone: "无人机",
  Dung: "粪",
  Dust: "粉尘",
  Electrophorus: "电鳗",
  Fertile: "肥沃",
  Fire: "火焰",
  Fragmented: "碎裂",
  Fungal: "真菌",
  Gas: "气体",
  Gem: "宝石",
  Golden: "金色",
  Green: "绿色",
  Hair: "毛发",
  High: "高级",
  Horn: "角",
  Horns: "角",
  Human: "人类",
  King: "王",
  Leech: "水蛭",
  Minion: "随从",
  Mistletoe: "槲寄生",
  Mosasaur: "沧龙",
  Node: "节点",
  Nodule: "结节",
  Nugget: "块",
  Offspring: "后代",
  Ore: "矿",
  Pack: "包",
  Paste: "膏",
  Pollen: "花粉",
  Power: "能量",
  Preserving: "防腐",
  Primal: "原始",
  Quality: "品质",
  Queen: "女王",
  Ragnarok: "诸神黄昏",
  Reaper: "死神",
  Red: "红色",
  Salt: "盐",
  Sap: "树脂",
  Slice: "切片",
  Substrate: "基质",
  Surface: "地表",
  Surprise: "惊喜",
  Titan: "泰坦",
  Unit: "单位",
  Wool: "羊毛",
  from: "来自",
  or: "或"
};

const DST_CODE_TERM_ZH: Record<string, string> = {
  asparagus: "芦笋",
  burnt: "烧毁",
  carrot: "胡萝卜",
  cave: "洞穴",
  corn: "玉米",
  deciduoustree: "桦栗树",
  double: "双",
  dragonfruit: "火龙果",
  durian: "榴莲",
  eggplant: "茄子",
  evergreen: "常青树",
  flower: "花",
  garlic: "大蒜",
  halloween: "万圣节",
  livingtree: "活木树",
  moon: "月亮",
  normal: "普通",
  old: "老",
  onion: "洋葱",
  oversized: "巨型",
  pepper: "辣椒",
  planted: "种植",
  pomegranate: "石榴",
  potato: "土豆",
  pumpkin: "南瓜",
  rose: "玫瑰",
  sapling: "树苗",
  short: "小型",
  sparse: "稀疏",
  stump: "树桩",
  tall: "大型",
  tomato: "番茄",
  tree: "树",
  triple: "三",
  twiggy: "多枝",
  watermelon: "西瓜",
  waxed: "上蜡"
};

const DST_CATEGORY_EN: Record<string, string> = {
  农业: "Farming",
  动物: "Creatures",
  基地: "Base",
  工具: "Tools",
  材料: "Materials",
  植物: "Plants",
  烹饪: "Cooking",
  矿石: "Rocks",
  礼物: "Gifts",
  种子: "Seeds",
  衣物: "Clothing",
  装备: "Equipment",
  道具: "Items",
  食物: "Food",
  魔法: "Magic"
};

const DST_PREFAB_NAME_EN: Record<string, string> = {
  bonestew: "Meaty Stew",
  cane: "Walking Cane",
  cutgrass: "Cut Grass",
  flint: "Flint",
  goldnugget: "Gold Nugget",
  log: "Logs",
  rocks: "Rocks",
  spider: "Spider",
  silk: "Silk",
  twigs: "Twigs"
};

type TermReplacement = readonly [pattern: RegExp, replacement: string];

const ARK_ITEM_BASE_REPLACEMENTS = buildTermReplacements({
  ...ARK_COMMON_TERM_ZH,
  ...ARK_ITEM_TERM_ZH
});
const ARK_ITEM_REPLACEMENTS = buildTermReplacements({
  ...ARK_CREATURE_NAME_ZH,
  ...ARK_COMMON_TERM_ZH,
  ...ARK_ITEM_TERM_ZH
});
const ARK_CREATURE_REPLACEMENTS = buildTermReplacements({
  ...ARK_COMMON_TERM_ZH,
  ...ARK_CREATURE_NAME_ZH,
  ...ARK_CREATURE_PREFIX_ZH
});
const REPEATED_UNKNOWN_PATTERNS = {
  生物: /(?:生物\s*){2,}/g,
  条目: /(?:条目\s*){2,}/g,
  分类: /(?:分类\s*){2,}/g
};

export function localizeArkCatalogName(option: ArkCatalogLike, locale: CatalogLocale): string {
  if (!isChineseLocale(locale)) {
    return option.name;
  }
  const dictionary = option.category === "Official" ? ARK_CREATURE_NAME_ZH : ARK_ITEM_NAME_ZH;
  if (dictionary[option.name]) {
    return dictionary[option.name];
  }
  const translated = option.category === "Official"
    ? localizeArkCreatureName(option.name)
    : localizeArkItemName(option.name);
  return option.category === "Official"
    ? sanitizeChineseCatalogText(translated, "\u751f\u7269", "\u751f\u7269")
    : sanitizeChineseCatalogText(
      translated,
      `${localizeArkCatalogCategory(option, "zh-CN")}\u6761\u76ee`,
      "\u6761\u76ee"
    );
}

export function localizeArkCatalogCategory(option: ArkCatalogLike, locale: CatalogLocale): string {
  const zhCategory = ARK_CATEGORY_ZH[option.category] ?? "\u5206\u7c7b";
  return selectLocaleText(locale, zhCategory, option.category);
}

export function arkCatalogSearchAliases(option: ArkCatalogLike): string {
  return [
    localizeArkCatalogName(option, "zh-CN"),
    localizeArkCatalogCategory(option, "zh-CN")
  ].filter(Boolean).join(" ");
}

export function localizeDstPrefabName(option: DstPrefabLike, locale: CatalogLocale): string {
  if (isChineseLocale(locale)) {
    return sanitizeChineseCatalogText(
      dstPrefabZhName(option),
      `${localizeDstPrefabCategory(option, "zh-CN")}\u6761\u76ee`,
      "\u6761\u76ee"
    );
  }
  return DST_PREFAB_NAME_EN[option.value] ?? formatGameCodeName(option.value);
}

export function localizeDstPrefabCategory(option: DstPrefabLike, locale: CatalogLocale): string {
  return localizeDstPrefabCategoryTerm(option.category, locale);
}

export function localizeDstPrefabCategoryTerm(category: string, locale: CatalogLocale): string {
  const zhCategory = sanitizeChineseCatalogText(category, "\u5206\u7c7b", "\u5206\u7c7b");
  return selectLocaleText(locale, zhCategory, DST_CATEGORY_EN[category] ?? formatGameCodeName(category));
}

export function dstPrefabSearchAliases(option: DstPrefabLike): string {
  return [
    localizeDstPrefabName(option, "zh-CN"),
    localizeDstPrefabCategory(option, "zh-CN"),
    localizeDstPrefabName(option, "en-US"),
    localizeDstPrefabCategory(option, "en-US")
  ].join(" ");
}

function localizeArkItemName(name: string): string {
  const parenthetical = name.match(/^(.*?)\s+\((.*?)\)$/);
  if (parenthetical) {
    const base = ARK_ITEM_NAME_ZH[parenthetical[1]]
      ?? replaceKnownTerms(expandCompoundArkName(parenthetical[1]), ARK_ITEM_BASE_REPLACEMENTS);
    const suffix = ARK_CREATURE_NAME_ZH[parenthetical[2]]
      ?? replaceKnownTerms(expandCompoundArkName(parenthetical[2]), ARK_ITEM_REPLACEMENTS);
    return `${base}\uFF08${suffix}\uFF09`;
  }
  return replaceKnownTerms(expandCompoundArkName(name), ARK_ITEM_REPLACEMENTS);
}

function localizeArkCreatureName(name: string): string {
  const parenthetical = name.match(/^(.*?)\s+\((.*?)\)$/);
  if (parenthetical) {
    const base = localizeArkCreatureName(parenthetical[1]);
    const suffix = ARK_PARENTHESES_ZH[parenthetical[2]] ?? parenthetical[2];
    return `${base}（${suffix}）`;
  }
  if (name.startsWith("X-")) {
    return `X种${localizeArkCreatureName(name.slice(2))}`;
  }
  if (name.startsWith("R-")) {
    return `R种${localizeArkCreatureName(name.slice(2))}`;
  }

  return replaceKnownTerms(expandCompoundArkName(name), ARK_CREATURE_REPLACEMENTS);
}

function buildTermReplacements(terms: Record<string, string>): TermReplacement[] {
  // Preserve longest-first replacement and dictionary override order for every catalog.
  return Object.keys(terms)
    .sort((left, right) => right.length - left.length)
    .map((term) => [new RegExp(`\\b${escapeRegExp(term)}\\b`, "g"), terms[term]]);
}

function replaceKnownTerms(value: string, replacements: readonly TermReplacement[]): string {
  let result = value;
  for (const [pattern, replacement] of replacements) {
    result = result.replace(pattern, replacement);
  }
  return collapseLocalizedSpaces(result);
}

function expandCompoundArkName(value: string): string {
  return value
    .replace(/_/g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
}

function dstPrefabZhName(option: DstPrefabLike): string {
  const parts = option.label.split(" / ");
  const display = parts.length >= 3 ? parts.slice(1, -1).join(" / ") : "";
  return display.trim() && display.trim() !== option.value
    ? display.trim()
    : localizeCodeTokens(option.value, DST_CODE_TERM_ZH, "\u6761\u76ee");
}

function sanitizeChineseCatalogText(
  value: string,
  fallback: string,
  normalizedUnknown: keyof typeof REPEATED_UNKNOWN_PATTERNS
): string {
  const unknownPattern = REPEATED_UNKNOWN_PATTERNS[normalizedUnknown];
  const sanitized = value
    .replace(/\{item\}/gi, "\u7269\u54c1")
    .replace(/[A-Za-z]{3,}/g, (match) => transliterateEnglishToken(match, normalizedUnknown))
    .replace(/\b[A-Za-z]{1,2}\b/g, (match) => (match === "X" || match === "R" ? match : ""))
    .replace(/[_.\/-]+/g, " ")
    .replace(/[()]/g, "")
    .replace(unknownPattern, normalizedUnknown)
    .replace(/\s+/g, " ")
    .trim();
  const collapsed = collapseLocalizedSpaces(sanitized).replace(unknownPattern, normalizedUnknown).trim();
  return /[\u3400-\u9fff]/.test(collapsed) ? collapsed : fallback;
}

function localizeCodeTokens(value: string, terms: Record<string, string>, fallback: string): string {
  const tokens = value.split(/[_./\s-]+/).filter(Boolean);
  if (tokens.length === 0) {
    return fallback;
  }
  return tokens.map((token) => terms[token.toLowerCase()] ?? transliterateEnglishToken(token, fallback)).join("");
}

function transliterateEnglishToken(token: string, fallback: string): string {
  const normalized = token.trim();
  if (!normalized) {
    return fallback;
  }
  const acronym = normalized.toUpperCase();
  if (acronym === "ARK") return "\u65b9\u821f";
  if (acronym === "BP") return "\u84dd\u56fe";
  if (acronym === "GFI") return "\u4ee3\u7801";
  if (acronym === "ID") return "\u7f16\u53f7";

  const syllables: Record<string, string> = {
    a: "\u963f",
    b: "\u5e03",
    c: "\u514b",
    d: "\u5fb7",
    e: "\u4f0a",
    f: "\u592b",
    g: "\u683c",
    h: "\u8d6b",
    i: "\u4f0a",
    j: "\u6770",
    k: "\u514b",
    l: "\u52d2",
    m: "\u59c6",
    n: "\u6069",
    o: "\u5965",
    p: "\u666e",
    q: "\u5947",
    r: "\u5c14",
    s: "\u65af",
    t: "\u7279",
    u: "\u4e4c",
    v: "\u7ef4",
    w: "\u7ef4",
    x: "\u514b\u65af",
    y: "\u4f0a",
    z: "\u5179"
  };
  return normalized
    .toLowerCase()
    .replace(/[^a-z]/g, "")
    .split("")
    .map((letter) => syllables[letter] ?? "")
    .join("") || fallback;
}

function formatGameCodeName(value: string): string {
  return value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/[_./-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .filter(Boolean)
    .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
    .join(" ") || value;
}

function collapseLocalizedSpaces(value: string): string {
  return value
    .replace(/([\u3400-\u9fff])\s+(?=[\u3400-\u9fff])/g, "$1")
    .replace(/\s+（/g, "（")
    .replace(/）\s+/g, "）")
    .trim();
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
