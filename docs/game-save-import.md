# 存档导入

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
