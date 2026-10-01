export interface ModuleStoreCopy {
  storeName: string;
  shortDescription: string;
  storyParagraphs?: string[];
  genres: string[];
  categories: string[];
  releaseDate: string;
  officialLinks?: {
    id: string;
    label: string;
    description: string;
    url: string;
    kind: string;
  }[];
}

export const EN_US_MODULE_STORE_COPY: Record<string, ModuleStoreCopy> = {
  abioticfactor: {
    storeName: "Abiotic Factor",
    shortDescription:
      "A 1-6 player co-op survival crafting game set in a paranormal research facility. Build tools, manage the base, and survive a containment failure with science on your side.",
    genres: ["Action", "Adventure", "RPG", "Simulation"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Jul 22, 2025"
  },
  arksurvivalascended: {
    storeName: "ARK: Survival Ascended",
    shortDescription:
      "A next-generation ARK survival experience rebuilt in Unreal Engine 5. Form a tribe, tame and breed prehistoric creatures, build, craft, and fight your way up the food chain.",
    genres: ["Action", "Adventure", "Massively Multiplayer", "RPG"],
    categories: ["Single-player", "Multi-player", "MMO", "Online PvP", "Online Co-op", "Cross-Platform Multiplayer", "Steam Achievements", "Family Sharing"],
    releaseDate: "Oct 25, 2023"
  },
  arksurvivalevolved: {
    storeName: "ARK: Survival Evolved",
    shortDescription:
      "Stranded on a mysterious island, players hunt, harvest, craft, build, and tame dinosaurs while surviving heat, cold, hunger, and other survivors.",
    genres: ["Action", "Adventure", "Indie", "Massively Multiplayer", "RPG"],
    categories: ["Single-player", "Multi-player", "MMO", "Online PvP", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop"],
    releaseDate: "Aug 27, 2017"
  },
  conanexiles: {
    storeName: "Conan Exiles",
    shortDescription:
      "An online multiplayer survival game set in the lands of Conan the Barbarian. Survive, build a kingdom, dominate enemies, and explore a brutal open world.",
    genres: ["Action", "Adventure", "Massively Multiplayer", "RPG", "Simulation"],
    categories: ["Single-player", "Multi-player", "MMO", "Online PvP", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop"],
    releaseDate: "May 8, 2018"
  },
  corekeeper: {
    storeName: "Core Keeper",
    shortDescription:
      "A mining sandbox adventure for 1-8 players. Explore an endless cavern, gather resources, build a base, fight bosses, farm, craft, and uncover the mystery of the ancient Core.",
    genres: ["Adventure", "Casual", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Aug 27, 2024"
  },
  humanitz: {
    storeName: "HumanitZ",
    shortDescription:
      "An isometric open-world survival sandbox set after a zombie outbreak. Scavenge, craft, build, fight, and keep a small group alive.",
    genres: ["Action", "Adventure", "RPG"],
    categories: ["Single-player", "Multi-player", "Online PvP", "Online Co-op", "Steam Achievements", "Partial controller support", "Family Sharing"],
    releaseDate: "Feb 6, 2026"
  },
  minecraft: {
    storeName: "Minecraft Java Edition",
    shortDescription:
      "An open-ended Java sandbox built around exploration, survival, building, and redstone engineering, where one world seed can grow into a long-running shared adventure.",
    storyParagraphs: [
      "Minecraft: Java Edition generates landscapes, caves, biomes, and structures for every world. Players can gather and survive from nothing, explore at their own pace, or use Creative mode to focus on architecture, machinery, and large-scale transformation.",
      "Its multiplayer appeal comes from shaping a world together: build settlements, travel through the Nether and the End, design automated farms and redstone systems, or establish a play style around the rules of your own community.",
      "The vanilla server supports data packs and resource packs. Mods or plugins require a deployment that is compatible with the intended loader or server distribution."
    ],
    genres: ["Adventure", "Sandbox", "Survival"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "LAN Co-op", "Dedicated Server"],
    releaseDate: "Nov 18, 2011",
    officialLinks: [
      {
        id: "minecraft-news",
        label: "Minecraft News",
        description: "Official Minecraft.net articles, update posts, and community-facing release coverage.",
        url: "https://www.minecraft.net/en-us/articles",
        kind: "News"
      },
      {
        id: "java-server-download",
        label: "Java server download",
        description: "Official Java Edition package download page from Minecraft.net.",
        url: "https://www.minecraft.net/en-us/download/server",
        kind: "Server jar"
      },
      {
        id: "java-server-setup",
        label: "Java server setup",
        description: "Official guide for preparing a Java Edition server and accepting the Minecraft EULA.",
        url: "https://help.minecraft.net/hc/en-us/articles/360058525452-How-to-Setup-a-Minecraft-Java-Edition-Server",
        kind: "Guide"
      },
      {
        id: "minecraft-eula",
        label: "Minecraft EULA",
        description: "Terms that apply to the official Minecraft server software.",
        url: "https://www.minecraft.net/en-us/eula",
        kind: "Terms"
      },
      {
        id: "release-changelogs",
        label: "Release changelogs",
        description: "Official Minecraft release changelogs for checking version behavior before updating a world.",
        url: "https://feedback.minecraft.net/hc/en-us/sections/360001186971-Release-Changelogs",
        kind: "Changelog"
      },
      {
        id: "snapshot-changelogs",
        label: "Snapshot changelogs",
        description: "Official snapshot and pre-release notes for operators who test ahead of stable Java releases.",
        url: "https://feedback.minecraft.net/hc/en-us/sections/360002267532-Snapshot-Information-and-Changelogs",
        kind: "Dev log"
      }
    ]
  },
  rust: {
    storeName: "Rust",
    shortDescription:
      "A harsh multiplayer survival game about gathering resources, building shelters, negotiating trust, raiding rivals, and enduring an unforgiving world.",
    genres: ["Action", "Adventure", "Indie", "Massively Multiplayer", "RPG"],
    categories: ["Multi-player", "MMO", "Online PvP", "Online Co-op", "Steam Achievements", "Steam Workshop"],
    releaseDate: "Feb 8, 2018"
  },
  rimworld: {
    storeName: "RimWorld",
    shortDescription:
      "A sci-fi colony simulator where survivors build a settlement, manage social conflicts, face raids and disasters, and tell emergent stories on the frontier.",
    genres: ["Indie", "Simulation", "Strategy"],
    categories: ["Single-player", "Steam Workshop", "Steam Achievements", "Steam Cloud", "Family Sharing"],
    releaseDate: "Oct 17, 2018",
    officialLinks: [
      {
        id: "rimworld-together-workshop",
        label: "RimWorld Together Workshop",
        description: "Client mod required for connecting to RimWorld Together servers.",
        url: "https://steamcommunity.com/sharedfiles/filedetails/?id=3005289691",
        kind: "Workshop"
      },
      {
        id: "rimworld-together-server",
        label: "RimWorld Together server releases",
        description: "Official RimWorld Together GitHub releases for GameServer.exe.",
        url: "https://github.com/RimWorld-Together/Rimworld-Together/releases",
        kind: "Server"
      }
    ]
  },
  satisfactory: {
    storeName: "Satisfactory",
    shortDescription:
      "A first-person factory-building game about exploration, logistics, automation, vertical construction, and expanding production across an alien planet.",
    genres: ["Adventure", "Indie", "Simulation", "Strategy"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support"],
    releaseDate: "Sep 10, 2024"
  },
  unturned: {
    storeName: "Unturned",
    shortDescription:
      "A free-to-play survival sandbox about scavenging, crafting, vehicles, maps, Workshop worlds, and staying alive after the zombie outbreak.",
    genres: ["Action", "Adventure", "Casual", "Free To Play", "Indie"],
    categories: ["Single-player", "Multi-player", "Online PvP", "Online Co-op", "LAN PvP", "LAN Co-op", "Steam Workshop"],
    releaseDate: "Jul 7, 2017"
  },
  dontstarve: {
    storeName: "Don't Starve Together",
    shortDescription:
      "Fight, farm, build, and explore together in the standalone multiplayer expansion to the uncompromising wilderness survival game Don't Starve.",
    genres: ["Action", "Adventure", "Indie", "Simulation", "Strategy"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop", "Family Sharing"],
    releaseDate: "Apr 21, 2016"
  },
  enshrouded: {
    storeName: "Enshrouded",
    shortDescription:
      "You are Flameborn, the last hope of a dying race. Survive a vast voxel world, fight bosses, build halls, and reclaim a kingdom lost to the Shroud.",
    genres: ["Action", "Adventure", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Jan 24, 2024"
  },
  necesse: {
    storeName: "Necesse",
    shortDescription:
      "Build, quest, and conquer across an infinite procedurally generated world. Play solo or with friends, settle villagers, explore dungeons, fight bosses, and craft gear.",
    genres: ["Action", "Adventure", "Casual", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Dec 12, 2019"
  },
  palworld: {
    storeName: "Palworld",
    shortDescription:
      "Collect mysterious creatures called Pals in a vast open world. Fight, build, farm, craft, automate production, and survive with friends.",
    genres: ["Action", "Adventure", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "Steam Achievements", "Full controller support", "Cross-Platform Multiplayer", "Family Sharing"],
    releaseDate: "Jan 19, 2024"
  },
  projectzomboid: {
    storeName: "Project Zomboid",
    shortDescription:
      "The ultimate zombie survival sandbox. Alone or in multiplayer, loot, build, craft, fight, farm, fish, and struggle to delay the inevitable.",
    genres: ["Indie", "RPG", "Simulation"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Workshop", "Steam Cloud", "Family Sharing"],
    releaseDate: "Nov 8, 2013"
  },
  sevendaystodie: {
    storeName: "7 Days to Die",
    shortDescription:
      "An open-world survival game that blends first-person shooter, survival horror, tower defense, and RPG systems in a brutal zombie sandbox.",
    genres: ["Action", "Adventure", "Indie", "RPG", "Simulation"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop", "Family Sharing"],
    releaseDate: "Jul 25, 2024"
  },
  terraria: {
    storeName: "Terraria",
    shortDescription:
      "Dig, fight, explore, and build in a side-scrolling adventure where the world is your canvas and action, discovery, and crafting drive progression.",
    genres: ["Action", "Adventure", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop", "Family Sharing"],
    releaseDate: "May 16, 2011"
  },
  valheim: {
    storeName: "Valheim",
    shortDescription:
      "A brutal exploration and survival game for 1-10 players set in a procedurally generated world inspired by Norse mythology.",
    genres: ["Action", "Adventure", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Feb 2, 2021"
  },
  vrising: {
    storeName: "V Rising",
    shortDescription:
      "Awaken as a vampire. Hunt for blood, avoid the sun, build a castle, gather allies, and conquer a gothic open world.",
    genres: ["Action", "Adventure", "Massively Multiplayer"],
    categories: ["Single-player", "Multi-player", "MMO", "Online PvP", "Online Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "May 8, 2024"
  }
};

export const ZH_CN_MODULE_STORE_COPY: Record<string, ModuleStoreCopy> = {
  abioticfactor: {
    storeName: "非生物因素",
    shortDescription:
      "1-6 人合作生存制作游戏，舞台是一座超自然研究设施。制作工具、经营基地，并用科学撑过收容失效后的混乱。",
    genres: ["动作", "冒险", "角色扮演", "模拟"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "跨平台多人", "Steam 成就", "完全支持控制器"],
    releaseDate: "2025 年 7 月 22 日"
  },
  arksurvivalascended: {
    storeName: "方舟：生存飞升",
    shortDescription:
      "使用虚幻引擎 5 重制的新一代 ARK 生存体验。组建部落、驯养并繁殖史前生物，在更重的资产与更大的世界中向食物链顶端推进。",
    genres: ["动作", "冒险", "大型多人在线", "角色扮演"],
    categories: ["单人", "多人", "大型多人在线", "在线 PvP", "在线合作", "跨平台多人", "Steam 成就", "家庭共享"],
    releaseDate: "2023 年 10 月 25 日"
  },
  arksurvivalevolved: {
    storeName: "方舟：生存进化",
    shortDescription:
      "在神秘岛屿上采集、制作、建造、狩猎并驯服恐龙，同时承受天气、饥渴、野生生物和其他幸存者的压力。",
    genres: ["动作", "冒险", "独立", "大型多人在线", "角色扮演"],
    categories: ["单人", "多人", "大型多人在线", "在线 PvP", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊"],
    releaseDate: "2017 年 8 月 27 日"
  },
  conanexiles: {
    storeName: "流放者柯南",
    shortDescription:
      "以《野蛮人柯南》荒野之地为背景的多人在线生存游戏。生存、建造王国、支配敌人，并探索残酷开放世界。",
    genres: ["动作", "冒险", "大型多人在线", "角色扮演", "模拟"],
    categories: ["单人", "多人", "大型多人在线", "在线 PvP", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊"],
    releaseDate: "2018 年 5 月 8 日"
  },
  corekeeper: {
    storeName: "护核纪元",
    shortDescription:
      "1-8 人挖矿沙盒冒险游戏。探索无尽洞窟、采集资源、建造基地、挑战 Boss、经营农场，并揭开古代核心的秘密。",
    genres: ["冒险", "休闲", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2024 年 8 月 27 日"
  },
  rimworld: {
    storeName: "环世界",
    shortDescription:
      "科幻殖民地模拟游戏，玩家在边境星球建设据点、管理居民关系、应对袭击与灾难，并通过 RimWorld Together 进行多人联机。",
    genres: ["独立", "模拟", "策略"],
    categories: ["单人", "Steam 创意工坊", "Steam 成就", "Steam 云", "家庭共享"],
    releaseDate: "2018 年 10 月 17 日"
  },
  humanitz: {
    storeName: "HumanitZ",
    shortDescription:
      "末日丧尸题材等距视角开放世界生存沙盒，围绕搜刮、制作、建造、战斗和小队生存展开。",
    genres: ["动作", "冒险", "角色扮演"],
    categories: ["单人", "多人", "在线 PvP", "在线合作", "Steam 成就", "部分支持控制器", "家庭共享"],
    releaseDate: "2026 年 2 月 6 日"
  },
  minecraft: {
    storeName: "Minecraft Java Edition",
    shortDescription:
      "一款以探索、生存、建造与红石工程为核心的 Java 方块沙盒；一张世界种子可以被一群玩家持续发展为长期共享的冒险世界。",
    storyParagraphs: [
      "Minecraft: Java Edition 的每个世界都会生成地形、洞穴、生物群系和结构。玩家既可以从零开始采集、生存与探索，也可以在创造模式中专注于建筑、机械和大规模改造。",
      "多人世界的魅力在于共同塑造：合作建设聚落、远征下界与末地、设计自动化农场和红石装置，或围绕社区规则发展属于自己服务器的玩法节奏。",
      "原版服务端支持数据包和资源包。希望使用模组或插件时，需要选择与目标加载器或服务端发行版兼容的部署方案。"
    ],
    genres: ["冒险", "沙盒", "生存"],
    categories: ["单人", "多人", "在线合作", "局域网合作", "专用服务器"],
    releaseDate: "2011 年 11 月 18 日",
    officialLinks: [
      {
        id: "minecraft-news",
        label: "Minecraft 官方新闻",
        description: "Minecraft.net 官方文章、更新说明和面向玩家的版本动态。",
        url: "https://www.minecraft.net/en-us/articles",
        kind: "新闻"
      },
      {
        id: "java-server-download",
        label: "Java 服务端下载",
        description: "Minecraft.net 提供的 Java Edition 官方下载页。",
        url: "https://www.minecraft.net/en-us/download/server",
        kind: "服务端 jar"
      },
      {
        id: "java-server-setup",
        label: "Java 服务端搭建指南",
        description: "用于准备 Java Edition 服务端并确认 Minecraft EULA 的官方说明。",
        url: "https://help.minecraft.net/hc/en-us/articles/360058525452-How-to-Setup-a-Minecraft-Java-Edition-Server",
        kind: "指南"
      },
      {
        id: "minecraft-eula",
        label: "Minecraft EULA",
        description: "适用于 Minecraft 官方服务端软件的使用条款。",
        url: "https://www.minecraft.net/en-us/eula",
        kind: "条款"
      },
      {
        id: "release-changelogs",
        label: "正式版更新日志",
        description: "用于确认版本行为、世界兼容性和更新变化的官方发布日志。",
        url: "https://feedback.minecraft.net/hc/en-us/sections/360001186971-Release-Changelogs",
        kind: "更新日志"
      },
      {
        id: "snapshot-changelogs",
        label: "快照 / 预发布日志",
        description: "给提前测试新 Java 版本的服主查看的官方快照、预发布和候选版记录。",
        url: "https://feedback.minecraft.net/hc/en-us/sections/360002267532-Snapshot-Information-and-Changelogs",
        kind: "开发日志"
      }
    ]
  },
  rust: {
    storeName: "Rust",
    shortDescription:
      "高压多人硬核生存游戏，围绕搜集资源、修建据点、建立信任、突袭对手和熬过危险环境展开。",
    genres: ["动作", "冒险", "独立", "大型多人在线", "角色扮演"],
    categories: ["多人", "大型多人在线", "在线 PvP", "在线合作", "Steam 成就", "Steam 创意工坊"],
    releaseDate: "2018 年 2 月 8 日"
  },
  satisfactory: {
    storeName: "Satisfactory",
    shortDescription:
      "第一人称工厂建造游戏，围绕探索、物流、自动化、立体建造和异星产能扩张展开。",
    genres: ["冒险", "独立", "模拟", "策略"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "完整控制器支持"],
    releaseDate: "2024 年 9 月 10 日"
  },
  unturned: {
    storeName: "Unturned",
    shortDescription:
      "免费生存沙盒，围绕搜刮、制作、载具、地图、Workshop 世界和末日求生展开。",
    genres: ["动作", "冒险", "休闲", "免费开玩", "独立"],
    categories: ["单人", "多人", "在线 PvP", "在线合作", "局域网 PvP", "局域网合作", "Steam 创意工坊"],
    releaseDate: "2017 年 7 月 7 日"
  },
  dontstarve: {
    storeName: "饥荒联机版",
    shortDescription:
      "在不妥协的荒野生存游戏多人版中一起战斗、耕种、建设和探索。",
    genres: ["动作", "冒险", "独立", "模拟", "策略"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊", "家庭共享"],
    releaseDate: "2016 年 4 月 21 日"
  },
  enshrouded: {
    storeName: "雾锁王国",
    shortDescription:
      "你是火焰之子，是濒死种族最后的希望。探索广阔体素世界、挑战强大 Boss、建造大厅，并夺回被瘴气吞没的王国。",
    genres: ["动作", "冒险", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2024 年 1 月 24 日"
  },
  necesse: {
    storeName: "奈斯启示录",
    shortDescription:
      "在程序生成的无限世界中建造、探索和征服。单人或与朋友一起定居、探索地下城、挑战 Boss，并制作装备。",
    genres: ["动作", "冒险", "休闲", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2019 年 12 月 12 日"
  },
  palworld: {
    storeName: "幻兽帕鲁",
    shortDescription:
      "在广阔开放世界中收集神奇生物帕鲁，让它们战斗、建造、务农、制作和自动化生产，并与朋友一起生存。",
    genres: ["动作", "冒险", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "Steam 成就", "完全支持控制器", "跨平台多人", "家庭共享"],
    releaseDate: "2024 年 1 月 19 日"
  },
  projectzomboid: {
    storeName: "僵尸毁灭工程",
    shortDescription:
      "开放式僵尸生存沙盒。单人或多人模式下，搜刮、建造、制作、战斗、种田和钓鱼，只为把不可避免的结局再往后拖一点。",
    genres: ["独立", "角色扮演", "模拟"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 创意工坊", "Steam 云", "家庭共享"],
    releaseDate: "2013 年 11 月 8 日"
  },
  sevendaystodie: {
    storeName: "七日杀",
    shortDescription:
      "开放世界生存游戏，融合第一人称射击、生存恐怖、塔防和角色扮演系统，构成残酷的丧尸沙盒。",
    genres: ["动作", "冒险", "独立", "角色扮演", "模拟"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊", "家庭共享"],
    releaseDate: "2024 年 7 月 25 日"
  },
  terraria: {
    storeName: "泰拉瑞亚",
    shortDescription:
      "挖掘、战斗、探索和建造。在这个横版冒险世界中，行动、发现和制作共同推动进程。",
    genres: ["动作", "冒险", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊", "家庭共享"],
    releaseDate: "2011 年 5 月 16 日"
  },
  valheim: {
    storeName: "英灵神殿",
    shortDescription:
      "1-10 人残酷探索生存游戏，舞台是受北欧神话启发的程序生成世界。",
    genres: ["动作", "冒险", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2021 年 2 月 2 日"
  },
  vrising: {
    storeName: "夜族崛起",
    shortDescription:
      "作为吸血鬼醒来，狩猎鲜血、避开阳光、建造城堡、招募盟友，并征服哥特开放世界。",
    genres: ["动作", "冒险", "大型多人在线"],
    categories: ["单人", "多人", "大型多人在线", "在线 PvP", "在线合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2024 年 5 月 8 日"
  }
};

const EN_US_RECENT_MODULE_STORE_COPY: Record<string, ModuleStoreCopy> = {
  barotrauma: {
    storeName: "Barotrauma",
    shortDescription:
      "A 2D co-op submarine simulator with survival horror and RPG systems. Crews run missions, maintain the vessel, fight monsters, and keep the campaign alive under pressure.",
    genres: ["Action", "Indie", "Simulation", "Strategy"],
    categories: ["Single-player", "Multi-player", "Online PvP", "Online Co-op", "Steam Achievements", "Steam Workshop", "Steam Cloud", "Family Sharing"],
    releaseDate: "Mar 13, 2023"
  },
  scum: {
    storeName: "SCUM",
    shortDescription:
      "A detailed open-world survival game with deep character simulation, looting, crafting, PvP pressure, and persistent multiplayer worlds.",
    genres: ["Action", "Adventure", "Indie", "Massively Multiplayer"],
    categories: ["Single-player", "Multi-player", "MMO", "Online PvP", "Online Co-op", "Steam Achievements", "Steam Trading Cards"],
    releaseDate: "Jun 17, 2025"
  },
  sonsoftheforest: {
    storeName: "Sons Of The Forest",
    shortDescription:
      "An open-world horror survival game where small groups craft, build, explore, and survive on an island full of hostile threats.",
    genres: ["Action", "Adventure", "Indie", "Simulation"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Steam Achievements", "Full controller support", "Steam Cloud", "Family Sharing"],
    releaseDate: "Feb 22, 2024"
  },
  soulmask: {
    storeName: "Soulmask",
    shortDescription:
      "A survival sandbox about ancient masks, tribe automation, recruitment, base building, and progression through a large open world.",
    genres: ["Adventure", "Indie", "RPG", "Simulation"],
    categories: ["Single-player", "Multi-player", "Online PvP", "Online Co-op", "LAN Co-op", "Steam Achievements", "Steam Workshop", "Steam Cloud"],
    releaseDate: "Apr 9, 2026"
  },
  squad: {
    storeName: "Squad",
    shortDescription:
      "A tactical first-person shooter built around teamwork, communication, 100-player battles, vehicles, base building, and integrated VOIP.",
    genres: ["Action", "Indie", "Massively Multiplayer", "Strategy"],
    categories: ["Multi-player", "Online PvP", "Online Co-op", "Steam Workshop", "Valve Anti-Cheat"],
    releaseDate: "Sep 23, 2020"
  },
  runescapedragonwilds: {
    storeName: "RuneScape: Dragonwilds",
    shortDescription:
      "A co-op survival crafting adventure set on Ashenfall, where players gather resources, build bases, master magic, and face dragons in the RuneScape universe.",
    genres: ["Action", "Adventure", "RPG", "Early Access"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Steam Achievements", "Family Sharing"],
    releaseDate: "Apr 15, 2025"
  },
  windrose: {
    storeName: "Windrose",
    shortDescription:
      "A PvE pirate-era survival adventure with sailing, building, crafting, boss fights, treasure hunts, and co-op exploration across land and sea.",
    genres: ["Action", "Adventure", "RPG", "Early Access"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Full controller support", "Steam Cloud", "Family Sharing"],
    releaseDate: "Apr 14, 2026"
  },
  returntomoria: {
    storeName: "The Lord of the Rings: Return to Moria",
    shortDescription:
      "A co-op survival crafting game set beneath the Misty Mountains, with persistent worlds, building, exploration, and dwarven expedition management.",
    genres: ["Action", "Adventure", "RPG", "Survival"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Aug 27, 2024"
  },
  astroneer: {
    storeName: "ASTRONEER",
    shortDescription:
      "A space sandbox about exploration, base building, automation, and co-op planetary survival across stylized worlds.",
    genres: ["Adventure", "Indie", "Simulation"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Feb 6, 2019"
  },
  theforest: {
    storeName: "The Forest",
    shortDescription:
      "A first-person survival horror game where co-op groups build, explore, craft, and defend against hostile inhabitants.",
    genres: ["Action", "Adventure", "Indie", "Simulation"],
    categories: ["Single-player", "Multi-player", "Online Co-op", "Steam Achievements", "Full controller support"],
    releaseDate: "Apr 30, 2018"
  },
  romestead: {
    storeName: "Romestead",
    shortDescription:
      "A 1-8 player survival town-building adventure about rebuilding a fallen Roman world, gathering resources, farming, fighting the dead, and restoring the gods.",
    genres: ["Action", "Adventure", "Indie", "RPG"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "LAN Co-op", "Steam Cloud", "Family Sharing"],
    releaseDate: "May 25, 2026"
  },
  nightingale: {
    storeName: "Nightingale",
    shortDescription:
      "A shared-world survival crafting adventure set across gaslamp fantasy realms. Build an estate, craft gear, fight Fae threats, and cross portals with friends.",
    genres: ["Action", "Adventure", "RPG", "Early Access"],
    categories: ["Single-player", "Multi-player", "Co-op", "Online Co-op", "Steam Achievements", "Full controller support", "Family Sharing"],
    releaseDate: "Feb 20, 2024"
  },
};

const ZH_CN_RECENT_MODULE_STORE_COPY: Record<string, ModuleStoreCopy> = {
  barotrauma: {
    storeName: "Barotrauma 潜渊症",
    shortDescription:
      "2D 合作潜艇模拟，结合生存恐怖和角色扮演。船员要执行任务、维修潜艇、对抗怪物，并把战役存档稳定跑下去。",
    genres: ["动作", "独立", "模拟", "策略"],
    categories: ["单人", "多人", "在线 PvP", "在线合作", "Steam 成就", "Steam 创意工坊", "Steam 云", "家庭共享"],
    releaseDate: "2023 年 3 月 13 日"
  },
  scum: {
    storeName: "人渣",
    shortDescription:
      "高细节开放世界生存游戏，包含深入角色模拟、搜刮、制作、PvP 压力和持久化多人世界。",
    genres: ["动作", "冒险", "独立", "大型多人在线"],
    categories: ["单人", "多人", "大型多人在线", "在线 PvP", "在线合作", "Steam 成就", "Steam 集换式卡牌"],
    releaseDate: "2025 年 6 月 17 日"
  },
  sonsoftheforest: {
    storeName: "Sons Of The Forest",
    shortDescription:
      "开放世界恐怖生存游戏，小团队在充满威胁的孤岛上制作、建造、探索并努力活下去。",
    genres: ["动作", "冒险", "独立", "模拟"],
    categories: ["单人", "多人", "在线合作", "Steam 成就", "完全支持控制器", "Steam 云", "家庭共享"],
    releaseDate: "2024 年 2 月 22 日"
  },
  soulmask: {
    storeName: "灵魂面甲",
    shortDescription:
      "围绕远古面甲、部落自动化、招募、建造和开放世界推进的生存沙盒。",
    genres: ["冒险", "独立", "角色扮演", "模拟"],
    categories: ["单人", "多人", "在线 PvP", "在线合作", "局域网合作", "Steam 成就", "Steam 创意工坊", "Steam 云"],
    releaseDate: "2026 年 4 月 9 日"
  },
  squad: {
    storeName: "Squad",
    shortDescription:
      "强调团队协作、持续沟通、百人战场、载具、基地建设和集成语音的战术第一人称射击。",
    genres: ["动作", "独立", "大型多人在线", "策略"],
    categories: ["多人", "在线 PvP", "在线合作", "Steam 创意工坊", "Valve 反作弊"],
    releaseDate: "2020 年 9 月 23 日"
  },
  runescapedragonwilds: {
    storeName: "RuneScape: Dragonwilds",
    shortDescription:
      "以 Ashenfall 为舞台的多人合作生存制作冒险，围绕采集、建造、魔法成长和对抗巨龙展开。",
    genres: ["动作", "冒险", "角色扮演", "抢先体验"],
    categories: ["单人", "多人", "在线合作", "Steam 成就", "家庭共享"],
    releaseDate: "2025 年 4 月 15 日"
  },
  windrose: {
    storeName: "Windrose: 风启之旅",
    shortDescription:
      "海盗时代 PvE 生存冒险，包含航海、建造、锻造、Boss 战、寻宝，以及海陆协作探索。",
    genres: ["动作", "冒险", "角色扮演", "抢先体验"],
    categories: ["单人", "多人", "在线合作", "完全支持控制器", "Steam 云", "家庭共享"],
    releaseDate: "2026 年 4 月 14 日"
  },
  returntomoria: {
    storeName: "The Lord of the Rings: Return to Moria",
    shortDescription:
      "中土矮人题材合作生存建造游戏，围绕持久世界、探索、采集、建筑和远征推进。",
    genres: ["动作", "冒险", "角色扮演", "生存"],
    categories: ["单人", "多人", "在线合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2024 年 8 月 27 日"
  },
  astroneer: {
    storeName: "ASTRONEER",
    shortDescription:
      "太空沙盒探索与基地建设游戏，支持自动化、星球探索和合作生存。",
    genres: ["冒险", "独立", "模拟"],
    categories: ["单人", "多人", "在线合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2019 年 2 月 6 日"
  },
  theforest: {
    storeName: "The Forest",
    shortDescription:
      "第一人称生存恐怖游戏，小队需要建造、探索、制造并抵御敌对威胁。",
    genres: ["动作", "冒险", "独立", "模拟"],
    categories: ["单人", "多人", "在线合作", "Steam 成就", "完全支持控制器"],
    releaseDate: "2018 年 4 月 30 日"
  },
  romestead: {
    storeName: "罗马斯泰德",
    shortDescription:
      "一款支持一到八人的生存建造冒险，玩家在古罗马风格的废土中采集资源、经营农田、扩建城镇、对抗亡者，并逐步重建神明与文明秩序。",
    genres: ["动作", "冒险", "独立", "角色扮演"],
    categories: ["单人", "多人", "合作", "在线合作", "局域网合作", "Steam 云", "家庭共享"],
    releaseDate: "2026 年 5 月 25 日"
  },
  nightingale: {
    storeName: "夜莺传说",
    shortDescription:
      "煤气灯奇幻风格的生存建造冒险，玩家穿行于传送门连接的界域，采集资源、建造据点、制作装备，并与好友合作探索。",
    genres: ["动作", "冒险", "角色扮演", "抢先体验"],
    categories: ["单人", "多人", "合作", "在线合作", "Steam 成就", "完全支持控制器", "家庭共享"],
    releaseDate: "2024 年 2 月 20 日"
  },
};

Object.assign(EN_US_MODULE_STORE_COPY, EN_US_RECENT_MODULE_STORE_COPY);
Object.assign(ZH_CN_MODULE_STORE_COPY, ZH_CN_RECENT_MODULE_STORE_COPY);

const ZH_CN_MODULE_DISPLAY_NAME_OVERRIDES: Record<string, string> = {
  astroneer: "异星探险家",
  barotrauma: "潜渊症 Barotrauma",
  rimworld: "环世界",
  humanitz: "人类Z HumanitZ",
  minecraft: "我的世界 Java 版",
  nightingale: "夜莺传说",
  returntomoria: "指环王：重返莫瑞亚",
  runescapedragonwilds: "符文世界：龙之荒野",
  rust: "腐蚀 Rust",
  satisfactory: "幸福工厂",
  sonsoftheforest: "森林之子",
  squad: "战术小队",
  theforest: "森林",
  unturned: "未转变者",
  windrose: "风启之旅 Windrose"
};

for (const [moduleId, storeName] of Object.entries(ZH_CN_MODULE_DISPLAY_NAME_OVERRIDES)) {
  const copy = ZH_CN_MODULE_STORE_COPY[moduleId];
  if (copy) {
    copy.storeName = storeName;
  }
}
