const fs = require("fs");
const path = require("path");
const { transpileTypeScript } = require("./typescript_source_tools.cjs");

const DESKTOP_ROOT = path.resolve(__dirname, "..");
const WORKSPACE_ROOT = path.resolve(DESKTOP_ROOT, "../..");
const MODULES_ROOT = path.join(WORKSPACE_ROOT, "modules");
const OUTPUT_DIR = path.join(DESKTOP_ROOT, "src", "i18n", "games");
const SETTINGS_REGISTRY_PATH = path.join(DESKTOP_ROOT, "src", "views", "settings", "module-registry.ts");
const CATALOG_CHUNK_SIZE = 2500;
const ARK_FIELD_DESCRIPTIONS = require(path.join(WORKSPACE_ROOT, "scripts", "ark_field_descriptions.json"));
const DST_SHARD_FIELD_COPY = {
  shard_layout: { title: "Shard layout", description: "Standard uses Master and optional Caves. Island Adventures uses Master, Caves, Islands and Volcano. Download and enable Island Adventures Core and Shipwrecked on all four shards first." },
  islands_enabled_workshop_mod_ids: { title: "Islands enabled Workshop Mod IDs", description: "One Workshop Mod ID per line. LanGame generates Islands/modoverrides.lua." },
  volcano_enabled_workshop_mod_ids: { title: "Volcano enabled Workshop Mod IDs", description: "One Workshop Mod ID per line. LanGame generates Volcano/modoverrides.lua." },
  islands_mod_configuration_options: { title: "Islands structured Mod configuration", description: "Store each Mod's configuration_options by Workshop ID for Islands/modoverrides.lua." },
  volcano_mod_configuration_options: { title: "Volcano structured Mod configuration", description: "Store each Mod's configuration_options by Workshop ID for Volcano/modoverrides.lua." },
  islands_modoverrides_lua: { title: "Advanced Islands modoverrides.lua", description: "A complete Lua file replaces the generated Islands Mod configuration. Structured Mod controls are disabled while this override is active." },
  volcano_modoverrides_lua: { title: "Advanced Volcano modoverrides.lua", description: "A complete Lua file replaces the generated Volcano Mod configuration. Structured Mod controls are disabled while this override is active." },
  islands_worldgenoverride_lua: { title: "Islands worldgenoverride.lua", description: "Leave empty to use the author's SURVIVAL_SHIPWRECKED_CLASSIC preset with a separate Volcano shard and no embedded volcano island. A complete Lua file replaces this generation configuration. Existing maps continue loading their save." },
  volcano_worldgenoverride_lua: { title: "Volcano worldgenoverride.lua", description: "Leave empty to use the author's SURVIVAL_VOLCANO_CLASSIC preset. A complete Lua file replaces this generation configuration. Existing maps continue loading their save." }
};

const SECTION_COPY = {
  room: {
    enTitle: "Room Settings",
    enDescription: "Identity, join password, player capacity, listing, and world selection.",
    zhTitle: "房间配置",
    zhDescription: "名称、简介、加入密码、人数、列表展示与地图或世界选择。"
  },
  network: {
    enTitle: "Network",
    enDescription: "Listen and public addresses, ports, and remote control connections.",
    zhTitle: "网络",
    zhDescription: "监听与公开地址、端口及远程控制连接。"
  },
  access: {
    enTitle: "Administration & Permissions",
    enDescription: "Operator credentials, permission levels, and admission policies.",
    zhTitle: "管理权限",
    zhDescription: "管理凭据、权限等级与准入策略。"
  },
  runtime: {
    enTitle: "Runtime & Advanced",
    enDescription: "Performance, process behavior, launch arguments, and advanced native overrides.",
    zhTitle: "运行与高级",
    zhDescription: "性能、进程行为、启动参数与高级原生覆盖。"
  },
  world: {
    enTitle: "World",
    enDescription: "World generation, difficulty, and gameplay rules.",
    zhTitle: "世界",
    zhDescription: "世界生成、难度和玩法规则。"
  },
  mastergen: {
    enTitle: "Overworld Generation",
    enDescription: "Overworld layout, resources, creatures, and fresh-world generation.",
    zhTitle: "\u5730\u8868\u4e16\u754c\u751f\u6210",
    zhDescription: "\u5730\u8868\u5730\u56fe\u3001\u8d44\u6e90\u3001\u751f\u7269\u4e0e\u65b0\u4e16\u754c\u751f\u6210\u89c4\u5219\u3002"
  },
  mastersettings: {
    enTitle: "Overworld Settings",
    enDescription: "Overworld seasons, events, survivor rules, and regrowth.",
    zhTitle: "\u5730\u8868\u4e16\u754c\u8bbe\u7f6e",
    zhDescription: "\u5730\u8868\u5b63\u8282\u3001\u6d3b\u52a8\u3001\u751f\u5b58\u8005\u89c4\u5219\u4e0e\u8d44\u6e90\u518d\u751f\u8bbe\u7f6e\u3002"
  },
  cavesgen: {
    enTitle: "Caves Generation",
    enDescription: "Caves layout, resources, creatures, and fresh-world generation.",
    zhTitle: "\u6d1e\u7a74\u4e16\u754c\u751f\u6210",
    zhDescription: "\u6d1e\u7a74\u5730\u56fe\u3001\u8d44\u6e90\u3001\u751f\u7269\u4e0e\u65b0\u4e16\u754c\u751f\u6210\u89c4\u5219\u3002"
  },
  cavessettings: {
    enTitle: "Caves Settings",
    enDescription: "Caves day cycle, threats, giants, and regrowth.",
    zhTitle: "\u6d1e\u7a74\u4e16\u754c\u8bbe\u7f6e",
    zhDescription: "\u6d1e\u7a74\u65e5\u671f\u5faa\u73af\u3001\u5a01\u80c1\u3001\u5de8\u517d\u4e0e\u8d44\u6e90\u518d\u751f\u8bbe\u7f6e\u3002"
  },
  admin: {
    enTitle: "Admin",
    enDescription: "Operator credentials and management permissions.",
    zhTitle: "管理",
    zhDescription: "管理凭据与管理权限。"
  },
  advanced: {
    enTitle: "Advanced",
    enDescription: "Low-level launch arguments and settings that should stay explicit.",
    zhTitle: "高级",
    zhDescription: "需要显式展示的底层启动参数和高级设置。"
  }
};

const EXACT_ZH_TITLES = {
  "Additional Map Servers": "附加地图服务器",
  "Admin Password": "管理员密码",
  "Admin Steam IDs": "管理员 Steam ID",
  "Auto Save Interval Seconds": "自动保存间隔（秒）",
  "Bind IP": "绑定 IP",
  "Difficulty": "难度",
  "Enable BattlEye": "启用 BattlEye",
  "Enable RCON": "启用 RCON",
  "Extra Launch Arguments": "额外启动参数",
  "Galaxy Name": "星系名称",
  "Join Password": "加入密码",
  "Map Name": "地图名称",
  "Max Players": "最大玩家数",
  "Network Compression Threshold": "网络压缩阈值",
  "Packet Rate Limit": "数据包速率上限",
  "Use Native Transport": "使用原生网络传输",
  "Accepts Transfers": "接受玩家跨服转移",
  "Owner GUID": "服主 GUID",
  "Public IP": "公网 IP",
  "RCON Password": "RCON 密码",
  "Save Interval Seconds": "保存间隔（秒）",
  "Server Description": "服务器描述",
  "Server Name": "服务器名称",
  "Server Password": "服务器密码",
  "World Name": "世界名称"
};

const WORD_ZH = {
  Account: "账号",
  Active: "启用",
  ActiveMods: "启用模组",
  Address: "地址",
  Admin: "管理员",
  Administrator: "管理员",
  Administrators: "管理员",
  Adventure: "冒险",
  AFK: "AFK",
  Add: "添加",
  Additional: "附加",
  All: "全部",
  Allow: "允许",
  Allowed: "允许",
  Altitude: "高度",
  Amount: "数量",
  Anti: "反",
  API: "API",
  App: "应用",
  Area: "区域",
  Anticheat: "反作弊",
  Auto: "自动",
  Automatic: "自动",
  Backup: "备份",
  Balance: "平衡",
  Base: "基础",
  Ban: "封禁",
  Banned: "封禁",
  BattlEye: "BattlEye",
  Block: "阻止",
  Blood: "鲜血",
  Boss: "Boss",
  Breeding: "繁殖",
  Bridge: "桥梁",
  Browser: "浏览器",
  Build: "建造",
  Building: "建筑",
  Broadcast: "广播",
  Camera: "镜头",
  Cap: "上限",
  Capacity: "容量",
  Category: "分类",
  Cavesgen: "洞穴生成",
  Cavessettings: "洞穴设置",
  Chat: "聊天",
  Check: "检查",
  Client: "客户端",
  Cluster: "集群",
  Collection: "合集",
  Combat: "战斗",
  Command: "命令",
  Commands: "命令",
  Community: "社区",
  Config: "配置",
  Connection: "连接",
  Container: "容器",
  Continence: "排泄",
  Cooldown: "冷却",
  Count: "数量",
  Costs: "成本",
  Craft: "制作",
  Crafting: "制作",
  Crate: "补给箱",
  Creative: "创造",
  Credentials: "凭据",
  Cross: "跨平台",
  Custom: "自定义",
  Damage: "伤害",
  Day: "白天",
  Days: "天数",
  Death: "死亡",
  Debug: "调试",
  Default: "默认",
  Delay: "延迟",
  Description: "描述",
  Difficulty: "难度",
  Direct: "直连",
  Disable: "禁用",
  Disabled: "禁用",
  Discovery: "发现",
  Discord: "Discord",
  Distance: "距离",
  Dino: "恐龙",
  Drop: "掉落",
  Durability: "耐久",
  Duration: "持续时间",
  Enable: "启用",
  Enabled: "启用",
  Enemy: "敌方",
  Enemies: "敌人",
  Engram: "印痕技能",
  Engrams: "印痕技能",
  Entry: "条目",
  Entries: "条目",
  Event: "事件",
  Events: "事件",
  Experience: "经验",
  Endpoint: "端点",
  Enforce: "强制",
  Extra: "额外",
  Farming: "耕作",
  Fatigue: "疲劳",
  File: "文件",
  Files: "文件",
  Filter: "过滤",
  Frequency: "频率",
  Food: "食物",
  Force: "强制",
  Friendly: "友方",
  Game: "游戏",
  Gamemode: "游戏模式",
  Gameplay: "玩法",
  Generation: "生成",
  Global: "全局",
  Grace: "宽限",
  Group: "分组",
  GUID: "GUID",
  Health: "生命",
  Hide: "隐藏",
  Host: "主机",
  Hunger: "饥饿",
  ID: "ID",
  IDs: "ID",
  Identity: "身份",
  Interval: "间隔",
  IP: "IP",
  Invisible: "隐形",
  Inventory: "背包",
  Item: "物品",
  Items: "物品",
  Join: "加入",
  Key: "键",
  Kick: "踢出",
  LAN: "局域网",
  Land: "土地",
  Locale: "语言区域",
  Level: "等级",
  Leveling: "等级",
  Limit: "限制",
  Limits: "限制",
  List: "名单",
  Log: "日志",
  Logs: "日志",
  Login: "登录",
  Loot: "战利品",
  Maintenance: "维护",
  Map: "地图",
  Mastergen: "主世界生成",
  Mastersettings: "主世界设置",
  Max: "最大",
  Maximum: "最大",
  Message: "消息",
  Min: "最小",
  Minimum: "最小",
  Mode: "模式",
  Moderator: "协管",
  Moderators: "协管",
  Moderation: "管理",
  Mods: "模组",
  Monster: "怪物",
  Monsters: "怪物",
  MOTD: "MOTD",
  Multiplier: "倍率",
  Multipliers: "倍率",
  Name: "名称",
  Network: "网络",
  NPC: "NPC",
  Online: "在线",
  Open: "开放",
  Option: "选项",
  Operations: "操作",
  Override: "覆盖",
  Owner: "服主",
  Password: "密码",
  Performance: "性能",
  Permission: "权限",
  Player: "玩家",
  Players: "玩家",
  Population: "人口",
  Point: "点",
  Points: "点数",
  Port: "端口",
  Power: "电力",
  Prevent: "阻止",
  Priority: "优先级",
  Progression: "进度",
  Prospects: "勘探任务",
  Public: "公开",
  Purge: "清剿",
  PvE: "PvE 模式",
  PVE: "PvE 模式",
  PvP: "PvP 模式",
  PVP: "PvP 模式",
  Query: "查询",
  Queue: "队列",
  Rate: "倍率",
  Rates: "倍率",
  Raw: "原始",
  RCON: "RCON",
  Refrigeration: "冷藏",
  Region: "区域",
  Reserved: "预留",
  Requirement: "需求",
  Replacements: "替换",
  Resistance: "抗性",
  Role: "角色",
  Room: "房间",
  Round: "回合",
  Respawn: "重生",
  Resource: "资源",
  Resources: "资源",
  Restart: "重启",
  Rule: "规则",
  Rules: "规则",
  Safety: "安全",
  Safehouses: "安全屋",
  Save: "存档",
  Saved: "保存",
  Score: "分数",
  Seconds: "秒",
  Seed: "种子",
  Security: "安全",
  Server: "服务器",
  Services: "服务",
  Session: "会话",
  Settings: "设置",
  Show: "显示",
  Slot: "槽位",
  Slots: "槽位",
  Sockets: "插座",
  Spawn: "生成",
  Spawns: "生成",
  Spoil: "腐坏",
  Startup: "启动",
  Steam: "Steam",
  Steam64: "Steam64",
  Structure: "建筑",
  Structures: "建筑",
  Surface: "表面",
  Survival: "生存",
  Tick: "Tick",
  Time: "时间",
  Timeout: "超时",
  Tainted: "污染",
  Thirst: "口渴",
  Token: "令牌",
  Transfer: "传送",
  Use: "使用",
  Vehicles: "载具",
  Wipe: "清档",
  Update: "更新",
  User: "用户",
  Users: "用户",
  VAC: "VAC",
  Visibility: "可见性",
  Voice: "语音",
  Vote: "投票",
  Voting: "投票",
  Welcome: "欢迎",
  Weather: "天气",
  Whitelist: "白名单",
  World: "世界",
  Workshop: "Workshop",
  XP: "经验"
};

const ENUM_ZH = {
  "": "服务器默认",
  "0": "值 0",
  "1": "值 1",
  "2": "值 2",
  "3": "值 3",
  admin: "管理员",
  all: "全部",
  always: "始终",
  any: "任意",
  anyone: "任何人",
  banned: "封禁",
  battleye: "BattlEye",
  canbedestroyedbyplayers: "可被玩家摧毁",
  canbedestroyedonlywhendecaying: "仅衰败时可摧毁",
  canbeseizedordestroyedbyplayers: "可夺取或摧毁",
  classic: "经典",
  competitive: "竞技",
  cooperative: "合作",
  creative: "创造",
  default: "默认",
  disabled: "禁用",
  easy: "简单",
  empty: "空",
  enabled: "启用",
  false: "否",
  fill: "填充",
  friends: "好友",
  few: "少量",
  hard: "困难",
  hardcore: "硬核",
  hidden: "隐藏",
  high: "高",
  insane: "极高",
  lan: "局域网",
  large: "大型",
  local: "本地",
  longseason: "长季节",
  match: "匹配",
  many: "大量",
  medium: "中等",
  mostly: "很多",
  never: "从不",
  no: "否",
  normal: "普通",
  none: "无",
  noseason: "无季节",
  off: "关闭",
  offline: "离线",
  often: "较多",
  on: "开启",
  online: "在线",
  pc: "PC",
  private: "私有",
  public: "公开",
  pve: "PvE",
  pvp: "PvP",
  pause: "暂停",
  rare: "稀少",
  relaxed: "休闲",
  roleplaying: "角色扮演",
  shortseason: "短季节",
  small: "小型",
  softcore: "软核",
  steamlan: "Steam 局域网",
  steamonline: "Steam 在线",
  survival: "生存",
  true: "是",
  uncommon: "较少",
  veryfast: "非常快",
  verylongseason: "超长季节",
  veryshortseason: "超短季节",
  veryslow: "非常慢",
  xbox: "Xbox"
};

function read(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function readTomlString(source, key) {
  const match = source.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, "m"));
  return match ? match[1] : "";
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasHan(value) {
  return /[\u3400-\u9fff]/u.test(value);
}

function isAcronym(value) {
  return /^[A-Z]{2,}$/u.test(value);
}

function splitEnglishWordParts(value) {
  return String(value).match(/[A-Z]{2,}(?=[A-Z][a-z]|$)|[A-Z]?[a-z]+|[0-9]+/gu) || [];
}

function splitIdentifierWords(value) {
  return String(value)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[\\/._-]+/g, " ")
    .replace(/[^A-Za-z0-9]+/g, " ")
    .trim()
    .split(/\s+/u)
    .filter(Boolean);
}

function resolveWordDictionary(value) {
  const direct = String(value);
  const upper = direct.toUpperCase();
  const lower = direct.toLowerCase();
  const extensionDictionary = typeof WORD_ZH_EXTENSIONS === "object" && WORD_ZH_EXTENSIONS !== null ? WORD_ZH_EXTENSIONS : {};
  return (
    WORD_ZH[direct] ??
    extensionDictionary[direct] ??
    WORD_ZH[upper] ??
    extensionDictionary[upper] ??
    WORD_ZH[lower] ??
    extensionDictionary[lower]
  );
}

function translateWordPiece(piece) {
  const direct = resolveWordDictionary(piece);
  if (direct) {
    return direct;
  }

  const subparts = splitEnglishWordParts(piece);
  const translated = subparts
    .map((subpart) => {
      const mapped = resolveWordDictionary(subpart);
      if (mapped) {
        return mapped;
      }
      if (/^\d+$/.test(subpart)) {
        return subpart;
      }
      if (isAcronym(subpart)) {
        return subpart;
      }
      return "";
    })
    .filter(Boolean)
    .join(" ")
    .trim();

  return translated || "";
}

function translateSourceKeyHint(rawKey) {
  const segment = normalizeSourceKey(rawKey).split(".").at(-1) ?? "";
  return splitIdentifierWords(segment).map(translateWordPiece).filter(Boolean).join(" ");
}

function translateEnglishText(value, fallback) {
  const source = String(value ?? "").trim();
  if (!source) {
    return typeof fallback === "string" && fallback.trim() ? fallback.trim() : "";
  }

  const pieces = source.match(/[A-Za-z0-9]+|[^A-Za-z0-9]+/g) || [];
  const translatedParts = [];

  for (const piece of pieces) {
    if (!piece) {
      continue;
    }
    if (!/^[A-Za-z0-9]+$/.test(piece)) {
      translatedParts.push(piece);
      continue;
    }

    const mapped = translateWordPiece(piece);
    if (mapped) {
      translatedParts.push(mapped);
      continue;
    }

    if (isAcronym(piece) || /^\d+$/.test(piece)) {
      translatedParts.push(piece);
    }
  }

  const normalized = translatedParts.join("").replace(/\s+/g, " ").trim();
  if (hasHan(normalized) || hasHan(source)) {
    return normalized;
  }

  return typeof fallback === "string" && fallback.trim() ? fallback.trim() : "";
}

function titleCaseWord(value) {
  if (!value) {
    return "";
  }
  const acronym = value.toUpperCase();
  if (["api", "fps", "guid", "id", "ip", "json", "pve", "pvp", "rcon", "udp", "url", "vac", "xp"].includes(value.toLowerCase())) {
    return acronym === "PVE" ? "PvE" : acronym === "PVP" ? "PvP" : acronym;
  }
  return value.charAt(0).toUpperCase() + value.slice(1).toLowerCase();
}

function humanizeKey(key) {
  return String(key)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .map(titleCaseWord)
    .join(" ");
}

function normalizeSourceKey(value) {
  return typeof value === "string" && value.trim() ? value.trim() : "";
}

function buildSchemaEnumOptionKey(value) {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function humanizeEnumValue(value) {
  if (typeof value === "string") {
    return humanizeKey(value.replace(/^-+/, "") || "Default");
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function tidyZhTitle(value) {
  return value
    .replace(/\s+/g, " ")
    .replace(/\s*\/\s*/g, " / ")
    .replace(/\s*\(\s*/g, "（")
    .replace(/\s*\)\s*/g, "）")
    .replace(/\s+/g, " ")
    .replace(/([\u3400-\u9fff])\s+(?=[\u3400-\u9fff])/gu, "$1")
    .trim();
}

function extractNativeDescription(value) {
  const source = typeof value === "string" ? value.trim() : "";
  const match = source.match(/Native description:\s*(.+?)\.?$/i);
  return match?.[1].trim().replace(/[。.]$/u, "") ?? "";
}

function extractNativeSettingLocation(value) {
  const source = typeof value === "string" ? value.trim() : "";
  return source.match(/^(.+?)\s+setting\s+[^.]+\.\s+Native description:/i)?.[1]?.trim() ?? "";
}

function translateEnglishDescription(rawDescription, context) {
  const source = typeof rawDescription === "string" ? rawDescription.trim() : "";
  if (!extractNativeDescription(source)) {
    return source;
  }

  const sourceKey = normalizeSourceKey(context.property["x-lsgm-source-key"]);
  const location = extractNativeSettingLocation(source);
  if (sourceKey && location) {
    return `Controls ${sourceKey} in ${location}.`;
  }
  if (sourceKey) {
    return `Controls the native ${sourceKey} setting for ${context.moduleName}.`;
  }
  return `Controls this native setting for ${context.moduleName}.`;
}

function stableNativeTitle(sourceTitle) {
  const normalized = sourceTitle.replace(/\s+/g, " ").trim();
  if (!normalized || normalized.length > 72) {
    return "服务器设置";
  }
  return `原生配置：${normalized}`;
}

function translateTitle(title, key, rawDescription = "") {
  const sourceTitle = typeof title === "string" && title.trim() ? title.trim() : humanizeKey(key);
  if (hasHan(sourceTitle)) {
    return sourceTitle;
  }

  const exact = EXACT_ZH_TITLES[sourceTitle] ?? Object.entries(EXACT_ZH_TITLES)
    .find(([source]) => source.toLowerCase() === sourceTitle.toLowerCase())?.[1];
  if (exact) {
    return exact;
  }

  const nativeDescription = extractNativeDescription(rawDescription);
  if (nativeDescription && hasHan(nativeDescription)) {
    return tidyZhTitle(nativeDescription);
  }

  const translated = translateEnglishText(sourceTitle, "");
  const result = tidyZhTitle(translated);
  return hasHan(result) ? result : stableNativeTitle(sourceTitle);
}

function translateDescription(rawDescription, context) {
  const source = typeof rawDescription === "string" ? rawDescription.trim() : "";
  const nativeDescription = extractNativeDescription(source);
  if (nativeDescription) {
    const sourceKey = normalizeSourceKey(context.property["x-lsgm-source-key"]);
    const location = extractNativeSettingLocation(source);
    return sourceKey && location
      ? `用于控制“${nativeDescription}”，对应 ${location} 中的 ${sourceKey}。`
      : sourceKey
        ? `用于控制“${nativeDescription}”，对应原生配置键 ${sourceKey}。`
        : `用于控制“${nativeDescription}”。`;
  }

  if (hasHan(source)) {
    return source;
  }

  const passedAs = source.match(/^Passed as (.+)\.?$/i);
  if (passedAs) {
    return `作为 ${passedAs[1].replace(/\.$/, "")} 传入参数。`;
  }

  const writtenTo = source.match(/^Written (?:directly )?to (.+)\.?$/i);
  if (writtenTo) {
    return `写入到 ${writtenTo[1].replace(/\.$/, "")}。`;
  }

  if (/one .* per line/i.test(source)) {
    return `每行对应一个 ${context.title.replace(/列表|条目$/u, "")} 条目。`;
  }

  const sourceKey = normalizeSourceKey(context.property["x-lsgm-source-key"]);
  const sourceSurface = normalizeSourceKey(context.property["x-lsgm-source-surface"]);

  if (sourceSurface === "launch_arg") {
    return sourceKey
      ? `将“${context.title}”作为 ${sourceKey} 启动参数传递给 ${context.moduleName}。`
      : `将“${context.title}”作为启动参数传递给 ${context.moduleName}。`;
  }

  if (sourceSurface === "generated_roster") {
    return sourceKey
      ? `将“${context.title}”设为玩家名单模式，并写入 ${sourceKey} 列表文件。`
      : `将“${context.title}”设为玩家名单模式，并写入玩家名单文件。`;
  }

  if (sourceSurface === "config_file") {
    return sourceKey
      ? `将“${context.title}”写入 ${context.moduleName} 配置文件中的 ${sourceKey}。`
      : `将“${context.title}”写入 ${context.moduleName} 配置文件。`;
  }

  if (sourceSurface === "materializer") {
    return sourceKey
      ? `根据“${context.title}”生成 ${sourceKey}，供 ${context.moduleName} 读取。`
      : `根据“${context.title}”生成配置，供 ${context.moduleName} 读取。`;
  }

  if (source) {
    return `配置 ${context.title}。${translateEnglishText(source, "用于该项的参数说明。")}`;
  }

  return `配置 ${context.title} 的补充说明(用于 ${context.moduleName} 配置页)。`;
}

function translateEnumLabel(value, label) {
  const raw = String(label ?? value ?? "").trim();
  const normalized = raw.replace(/^-+/, "").replace(/[^A-Za-z0-9]+/g, "").toLowerCase();
  if (ENUM_ZH[normalized] !== undefined) {
    return ENUM_ZH[normalized];
  }
  if (ENUM_ZH[raw.toLowerCase()] !== undefined) {
    return ENUM_ZH[raw.toLowerCase()];
  }
  const translated = translateEnglishText(raw || "Option", "选项");
  if (hasHan(translated)) {
    return translated;
  }

  const optionHint = translateSourceKeyHint(raw);
  return optionHint ? `选项 ${optionHint}` : "选项";
}

function addSchemaEntries(enCatalog, zhCatalog, moduleInfo, schema) {
  const properties = isRecord(schema.properties) ? schema.properties : {};
  const sectionIds = new Set();

  for (const [fieldKey, property] of Object.entries(properties)) {
    if (fieldKey === "bind_ip" || !isRecord(property)) {
      continue;
    }

    const baseKey = `settings.schema.${moduleInfo.id}.${fieldKey}`;
    const dstCopy = moduleInfo.id === "dontstarve" ? DST_SHARD_FIELD_COPY[fieldKey] : undefined;
    const enTitle = dstCopy?.title ?? (typeof property.title === "string" && property.title.trim()
      ? property.title.trim()
      : humanizeKey(fieldKey));
    const zhTitle = dstCopy ? property.title : translateTitle(enTitle, fieldKey, property.description);
    enCatalog[`${baseKey}.title`] = enTitle;
    zhCatalog[`${baseKey}.title`] = zhTitle;

    if (typeof property.description === "string" && property.description.trim()) {
      enCatalog[`${baseKey}.description`] = dstCopy?.description ?? translateEnglishDescription(property.description, {
        moduleName: moduleInfo.displayName,
        property
      });
      const authoredHelp = ARK_FIELD_DESCRIPTIONS[moduleInfo.id]?.[fieldKey];
      if (authoredHelp && authoredHelp.en !== property.description) {
        throw new Error(`ARK help differs from its source: ${moduleInfo.id}.${fieldKey}`);
      }
      zhCatalog[`${baseKey}.description`] = dstCopy ? property.description : authoredHelp?.["zh-CN"] ?? translateDescription(property.description, {
        moduleName: moduleInfo.displayName,
        property,
        title: zhTitle
      });
    }

    if (Array.isArray(property.enum)) {
      const labels = Array.isArray(property.enum_labels) ? property.enum_labels : [];
      for (const [index, optionValue] of property.enum.entries()) {
        const optionKey = `${baseKey}.option.${buildSchemaEnumOptionKey(optionValue)}`;
        const label = typeof labels[index] === "string" ? labels[index] : humanizeEnumValue(optionValue);
        enCatalog[optionKey] = label;
        zhCatalog[optionKey] = translateEnumLabel(optionValue, label);
      }
    }

    if (typeof property["x-lsgm-section"] === "string" && property["x-lsgm-section"].trim()) {
      sectionIds.add(property["x-lsgm-section"].trim());
    }
  }

  for (const sectionId of sectionIds) {
    const copy = SECTION_COPY[sectionId] ?? {
      enTitle: humanizeKey(sectionId),
      enDescription: `${moduleInfo.displayName} ${humanizeKey(sectionId).toLowerCase()} settings.`,
      zhTitle: translateTitle(humanizeKey(sectionId), sectionId),
      zhDescription: `${moduleInfo.displayName} 的${translateTitle(humanizeKey(sectionId), sectionId)}设置。`
    };
    enCatalog[`${moduleInfo.id}.settings.sections.${sectionId}`] = copy.enTitle;
    enCatalog[`${moduleInfo.id}.settings.sections.${sectionId}Description`] = copy.enDescription;
    zhCatalog[`${moduleInfo.id}.settings.sections.${sectionId}`] = copy.zhTitle;
    zhCatalog[`${moduleInfo.id}.settings.sections.${sectionId}Description`] = copy.zhDescription;
    enCatalog[`${moduleInfo.id}.settings.groups.${sectionId}.title`] = copy.enTitle;
    enCatalog[`${moduleInfo.id}.settings.groups.${sectionId}.description`] = copy.enDescription;
    zhCatalog[`${moduleInfo.id}.settings.groups.${sectionId}.title`] = copy.zhTitle;
    zhCatalog[`${moduleInfo.id}.settings.groups.${sectionId}.description`] = copy.zhDescription;
  }
}

function collectModules() {
  return fs
    .readdirSync(MODULES_ROOT, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .filter((id) => fs.existsSync(path.join(MODULES_ROOT, id, "schema.json")))
    .sort()
    .map((id) => {
      const moduleTomlPath = path.join(MODULES_ROOT, id, "module.toml");
      const moduleToml = fs.existsSync(moduleTomlPath) ? read(moduleTomlPath) : "";
      return {
        id,
        displayName: readTomlString(moduleToml, "name") || humanizeKey(id)
      };
    });
}

function registerTypeScriptForSettingsIntrospection() {
  function compileTypeScript(module, filename) {
    const source = read(filename);
    module._compile(transpileTypeScript(source, filename), filename);
  }

  require.extensions[".ts"] = compileTypeScript;
  require.extensions[".tsx"] = compileTypeScript;
  require.extensions[".css"] = function compileEmptyCss(module, filename) {
    module._compile("", filename);
  };
}

function fieldControlForSchemaProperty(property) {
  if (Array.isArray(property.enum)) {
    return "select";
  }
  if (property.format === "textarea") {
    return "textarea";
  }
  if (property.type === "boolean") {
    return "checkbox";
  }
  if (property.type === "integer" || property.type === "number") {
    return "number";
  }
  return "text";
}

function buildGuidedFields(schema) {
  const properties = isRecord(schema.properties) ? schema.properties : {};

  return Object.entries(properties).map(([key, property]) => ({
    key,
    title: typeof property.title === "string" && property.title.trim() ? property.title.trim() : humanizeKey(key),
    description: typeof property.description === "string" ? property.description : "",
    type:
      property.type === "integer"
        ? "integer"
        : property.type === "number"
          ? "number"
          : property.type === "boolean"
            ? "boolean"
            : "string",
    control: fieldControlForSchemaProperty(property),
    sectionId: typeof property["x-lsgm-section"] === "string" && property["x-lsgm-section"].trim()
      ? property["x-lsgm-section"].trim()
      : "advanced",
    required: false,
    enumOptions: Array.isArray(property.enum)
      ? property.enum.map((value) => ({ label: humanizeEnumValue(value), value }))
      : undefined
  }));
}

function titleFromSettingsKey(key) {
  const parts = String(key).split(".");
  const last = parts.at(-1);
  if (last === "title" || last === "description") {
    return humanizeKey(parts.at(-2) ?? key);
  }
  if (last && last.endsWith("Description")) {
    return humanizeKey(last.slice(0, -"Description".length));
  }
  return humanizeKey(last ?? key);
}

function exactRuntimeSectionCopy(key) {
  const match = String(key).match(/\.settings\.sections\.(mastergen|mastersettings|cavesgen|cavessettings)(Description)?$/);
  if (!match) return null;
  return { copy: SECTION_COPY[match[1]], description: Boolean(match[2]) };
}

function englishSettingsFallback(key, fallback) {
  const section = exactRuntimeSectionCopy(key);
  if (section) {
    return section.description ? section.copy.enDescription : section.copy.enTitle;
  }
  if (typeof fallback === "string" && fallback.trim()) {
    return fallback.trim();
  }
  return titleFromSettingsKey(key);
}

function chineseSettingsFallback(key, fallback) {
  const section = exactRuntimeSectionCopy(key);
  if (section) {
    return section.description ? section.copy.zhDescription : section.copy.zhTitle;
  }
  const english = englishSettingsFallback(key, fallback);
  const title = translateTitle(
    key.endsWith(".description") || key.endsWith("Description")
      ? titleFromSettingsKey(key)
      : english,
    key
  );

  if (key.endsWith(".description") || key.endsWith("Description")) {
    return `“${title}”分组中的设置，保存后会应用到当前游戏实例。`;
  }

  return title;
}

function addGeneratedSettingsEntry(enCatalog, zhCatalog, key, fallback) {
  if (typeof key !== "string" || !key.includes(".")) {
    return;
  }
  // A missing field description is optional help, not a group label to invent.
  if (key.startsWith("settings.schema.") && key.endsWith(".description") &&
      (typeof fallback !== "string" || !fallback.trim())) {
    return;
  }

  if (enCatalog[key] === undefined) {
    enCatalog[key] = englishSettingsFallback(key, fallback);
  }
  if (zhCatalog[key] === undefined) {
    zhCatalog[key] = chineseSettingsFallback(key, fallback);
  }
}

function addRuntimeSettingsEntries(enCatalog, zhCatalog, modules, schemasByModuleId) {
  registerTypeScriptForSettingsIntrospection();
  const { resolveSettingsModuleDefinition } = require(SETTINGS_REGISTRY_PATH);

  for (const moduleInfo of modules) {
    const definition = resolveSettingsModuleDefinition(moduleInfo.id);
    const schema = schemasByModuleId.get(moduleInfo.id);
    if (!definition || !schema) {
      continue;
    }

    const t = (key, _params, fallback) => {
      addGeneratedSettingsEntry(enCatalog, zhCatalog, key, fallback);
      return typeof fallback === "string" ? fallback : String(key ?? "");
    };
    const sections = definition.getSections ? definition.getSections(t, "zh-CN") : (definition.sections ?? []);
    const fields = buildGuidedFields(schema);

    for (const section of sections) {
      const sectionFields = fields.filter((field) => field.sectionId === section.id);
      definition.buildFieldGroups?.(section.id, sectionFields, "zh-CN", t);

      for (const field of sectionFields) {
        definition.getFieldCopy?.(field.key, t, "zh-CN");

        for (const option of field.enumOptions ?? []) {
          definition.getEnumOptionLabel?.(field.key, option.value, "zh-CN", t);
        }
      }
    }
  }
}

function formatCatalogChunk(exportName, entries) {
  const body = entries
    .map(([key, value]) => `  ${JSON.stringify(key)}: ${JSON.stringify(value)}`)
    .join(",\n");

  return `import type { MessageCatalog } from "../../i18n-config";\n\n` +
    `// Generated from modules/*/schema.json by generate_i18n_schema_catalogs.cjs.\n` +
    `export const ${exportName}: MessageCatalog = {\n${body}\n};\n`;
}

function formatCatalogIndex(exportName, chunks) {
  const imports = chunks
    .map((chunk) => `import { ${chunk.exportName} } from "./${chunk.fileStem}";`)
    .join("\n");
  const spreads = chunks.map((chunk) => `  ...${chunk.exportName},`).join("\n");
  const importBlock = imports ? `${imports}\n` : "";
  const body = spreads ? `${spreads}\n` : "";

  return `import type { MessageCatalog } from "../../i18n-config";\n` +
    importBlock +
    `\n// Generated from modules/*/schema.json by generate_i18n_schema_catalogs.cjs.\n` +
    `// Hand-authored game catalogs are spread after this catalog and may override any entry here.\n` +
    `export const ${exportName}: MessageCatalog = {\n${body}};\n`;
}

function removeGeneratedCatalogChunks(baseName) {
  const prefix = `${baseName}.chunk-`;
  for (const fileName of fs.readdirSync(OUTPUT_DIR)) {
    if (fileName.startsWith(prefix) && fileName.endsWith(".ts")) {
      fs.unlinkSync(path.join(OUTPUT_DIR, fileName));
    }
  }
}

function writeCatalog(fileName, exportName, catalog) {
  const baseName = fileName.replace(/\.ts$/, "");
  const entries = Object.entries(catalog).sort(([left], [right]) => left.localeCompare(right));
  const chunks = [];

  removeGeneratedCatalogChunks(baseName);
  for (let index = 0; index < entries.length; index += CATALOG_CHUNK_SIZE) {
    const chunkNumber = chunks.length + 1;
    const chunkSuffix = String(chunkNumber).padStart(2, "0");
    const chunkFileStem = `${baseName}.chunk-${chunkSuffix}`;
    const chunkExportName = `${exportName}_CHUNK_${chunkSuffix}`;
    const chunkEntries = entries.slice(index, index + CATALOG_CHUNK_SIZE);
    fs.writeFileSync(
      path.join(OUTPUT_DIR, `${chunkFileStem}.ts`),
      formatCatalogChunk(chunkExportName, chunkEntries),
      "utf8"
    );
    chunks.push({ exportName: chunkExportName, fileStem: chunkFileStem });
  }

  fs.writeFileSync(path.join(OUTPUT_DIR, fileName), formatCatalogIndex(exportName, chunks), "utf8");
}

function mergeModuleCatalog(existing, generated, moduleIds) {
  const prefixes = moduleIds.flatMap((id) => [`settings.schema.${id}.`, `${id}.settings.`]);
  return {
    ...Object.fromEntries(Object.entries(existing).filter(([key]) => !prefixes.some((prefix) => key.startsWith(prefix)))),
    ...generated
  };
}

function readGeneratedCatalog(baseName) {
  const catalog = {};
  const chunks = fs.readdirSync(OUTPUT_DIR).filter((name) => name.startsWith(`${baseName}.chunk-`) && name.endsWith(".ts"));
  if (!chunks.length) throw new Error(`No existing ${baseName} chunks for a scoped generation.`);
  for (const name of chunks.sort()) {
    const source = read(path.join(OUTPUT_DIR, name));
    for (const [key, value] of Object.entries(parseGeneratedChunk(source))) {
      if (Object.hasOwn(catalog, key)) throw new Error(`Duplicate generated message: ${key}`);
      catalog[key] = value;
    }
  }
  return catalog;
}

function parseGeneratedChunk(source) {
  const body = source.match(/export const \w+: MessageCatalog = \{\r?\n([\s\S]*?)\r?\n\};\s*$/);
  if (!body) throw new Error("Invalid generated message chunk.");
  const catalog = {};
  for (const line of body[1].split(/\r?\n/)) {
    if (!line.trim()) continue;
    const match = line.match(/^\s*("(?:\\.|[^"\\])*"):\s*("(?:\\.|[^"\\])*"),?\s*$/);
    if (!match) throw new Error("Invalid generated message entry; refusing a partial scoped generation.");
    const key = JSON.parse(match[1]);
    if (Object.hasOwn(catalog, key)) throw new Error(`Duplicate generated message: ${key}`);
    catalog[key] = JSON.parse(match[2]);
  }
  return catalog;
}

function main() {
  let enCatalog = {};
  let zhCatalog = {};
  const allModules = collectModules();
  const requestedModules = new Set();
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index += 2) {
    const id = args[index + 1];
    if (args[index] !== "--module" || !allModules.some((entry) => entry.id === id)) {
      throw new Error("Use --module <existing-module-id> for a scoped generation, or no arguments for all modules.");
    }
    requestedModules.add(id);
  }
  const modules = requestedModules.size ? allModules.filter(({ id }) => requestedModules.has(id)) : allModules;
  const schemasByModuleId = new Map();

  for (const moduleInfo of modules) {
    const schemaPath = path.join(MODULES_ROOT, moduleInfo.id, "schema.json");
    const schema = JSON.parse(read(schemaPath));
    schemasByModuleId.set(moduleInfo.id, schema);
    addSchemaEntries(enCatalog, zhCatalog, moduleInfo, schema);
  }

  addRuntimeSettingsEntries(enCatalog, zhCatalog, modules, schemasByModuleId);

  if (requestedModules.size) {
    enCatalog = mergeModuleCatalog(readGeneratedCatalog("schema-generated.en"), enCatalog, [...requestedModules]);
    zhCatalog = mergeModuleCatalog(readGeneratedCatalog("schema-generated.zh-cn"), zhCatalog, [...requestedModules]);
  }

  writeCatalog("schema-generated.en.ts", "EN_US_SCHEMA_GENERATED_MESSAGES", enCatalog);
  writeCatalog("schema-generated.zh-cn.ts", "ZH_CN_SCHEMA_GENERATED_MESSAGES", zhCatalog);
  console.log(
    `Generated ${Object.keys(enCatalog).length} en-US schema messages and ${Object.keys(zhCatalog).length} zh-CN schema messages.`
  );
}

if (require.main === module) main();
module.exports = { addGeneratedSettingsEntry, mergeModuleCatalog, parseGeneratedChunk };
