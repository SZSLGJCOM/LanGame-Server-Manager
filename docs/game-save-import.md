# 存档导入 / Save import

[简体中文](#简体中文) | [English](#english)

## 简体中文

在服务器实例的 **维护 → 导入存档** 中选择来源文件并检查导入内容。

32 款游戏中，31 款支持已有专服存档迁入；Squad 没有可迁入的持久世界。

| 游戏 | 导入范围与必要条件 |
| --- | --- |
| ARK ASE / ASA | 当前地图、玩家和部落；来源服务器 ID 映射到目标实例，集群数据另行处理 |
| Conan Exiles | 原生 game_0.db 或旧 Game.db；由游戏执行版本升级 |
| HumanitZ | 完整 SaveGames，匹配 SaveName |
| Nightingale | Offline 世界、Profiles 及配套 CurrentDeploy 选择 |
| Project Zomboid | 多人世界与角色记录；登录账户库不迁入，使用原用户名及 SteamID |
| SCUM | 原生专服 SaveFiles 数据库 |
| Soulmask | Level01_Main 专服世界，其他地图转换另行处理 |
| RimWorld Together | 完整 Assets，保留用户名与聚落关联 |
| Rust | 世界、地图与玩家数据库；保留目标 cfg 和 Rust+ 身份 |
| Satisfactory | 世界 .sav；启动后经游戏内 Server Manager 选择加载 |
| 7 Days to Die | 选中世界与配套 GeneratedWorlds；匹配真实地图名和存档名 |
| Sons of the Forest / The Forest | 选中专服槽位；启动方式必须为 Continue |
| Unturned | 成套 Level 与 Players，匹配地图 |
| Windrose | 停服后的完整 Default；配置与原生世界 ID 一致 |

另外支持以下 15 款游戏的专服存档导入：Abiotic Factor、Astroneer、Barotrauma、Core Keeper、Don't Starve Together、Enshrouded、Minecraft Java、Necesse、Palworld、Return to Moria、Romestead、RuneScape: Dragonwilds、Terraria、Valheim、V Rising。

单机转专服的限制单独显示：ARK、Palworld、HumanitZ 等原房主身份不会自动转换；SCUM 单机转换不支持；Nightingale 的单机迁移按游戏自己的流程进行。支持本地世界文件迁入，也不代表版本、Mod 或角色身份已完成转换。

导入要求双方停服，检查后确认；自动建立可恢复的导入前备份，保留来源及目标服务器配置。七日杀世界和生成地图分别备份、分别计算保留数量，失败时两个导入范围一起回滚。布局和文件校验不能替代启动游戏后核对世界与玩家。

逐游戏限制与原始资料保留在 [存档支持数据](../apps/desktop/src/save-import/guides.json)，界面按当前游戏展示。

## English

Choose the source files in the server instance under **Maintenance → Import save** and review what will be imported.

Of the 32 games, 31 support importing existing dedicated-server saves; Squad has no persistent world to import.

| Game | Import scope and requirements |
| --- | --- |
| ARK ASE / ASA | Current map, players and tribes; source server IDs are mapped to the target instance, cluster data is handled separately |
| Conan Exiles | Native game_0.db or legacy Game.db; the game performs the version upgrade |
| HumanitZ | Complete SaveGames matching SaveName |
| Nightingale | Offline world, Profiles and the matching CurrentDeploy selection |
| Project Zomboid | Multiplayer world and character records; the login account database is not imported, original usernames and SteamIDs are used |
| SCUM | Native dedicated-server SaveFiles database |
| Soulmask | Level01_Main dedicated world; other map conversion is handled separately |
| RimWorld Together | Complete Assets, preserving username and settlement association |
| Rust | World, map and player databases; target cfg and Rust+ identity are preserved |
| Satisfactory | World .sav; loaded through the in-game Server Manager after startup |
| 7 Days to Die | Selected world and matching GeneratedWorlds; real map and save names must match |
| Sons of the Forest / The Forest | Selected dedicated slot; start mode must be Continue |
| Unturned | Matching Level and Players set for the map |
| Windrose | Complete Default after the server stops; configuration and native world ID must match |

Dedicated-server save import is also supported for these 15 games: Abiotic Factor, Astroneer, Barotrauma, Core Keeper, Don't Starve Together, Enshrouded, Minecraft Java, Necesse, Palworld, Return to Moria, Romestead, RuneScape: Dragonwilds, Terraria, Valheim, V Rising.

Single-player-to-dedicated limits are shown separately: original host identities for ARK, Palworld, HumanitZ and others are not converted automatically; SCUM single-player conversion is not supported; Nightingale single-player migration follows the game's own process. Support for importing local world files does not mean versions, Mods or character identities have been converted.

Import requires both servers to be stopped and is confirmed after review. A recoverable pre-import backup is created automatically and source and target server configurations are preserved. 7 Days to Die worlds and generated maps are backed up and retained separately, and both scopes roll back together on failure. Layout and file checks do not replace verifying the world and players after starting the game.

Per-game limits and original references are kept in the [save support data](../apps/desktop/src/save-import/guides.json); the interface shows the entry for the current game.
