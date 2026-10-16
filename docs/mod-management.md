# Mod 管理 / Mod management

## 简体中文

进入实例的 **Mod → 我的 Mod** 管理该实例中的文件。Nexus Mods、Thunderstore、CurseForge 等来源的本地包导入后使用同一套文件管理操作；来源网站下载到电脑的文件仍需先导入实例。游戏原生管理的 Workshop、CurseForge ID 和启用列表沿用各游戏已有的操作，不删除共享下载缓存。

- **纳入管理**：为此前已放入 Mod 目录的包建立文件归属记录。只有可识别的 Mod 载荷或包标记才能纳管；官方文件、加载器、配置目录和无关文件不能作为普通 Mod 接管。
- **禁用 / 启用**：停止服务器后，把包移出游戏的加载目录，或者恢复到原位置。同名位置已有其他内容时拒绝覆盖。禁用的包继续显示在“我的 Mod”中。
- **删除**：先检查依赖，再移出加载目录。只清理归属记录中内容未变的非配置文件；配置、存档、新增文件、用户修改过的文件保留在恢复位置，可从操作结果打开。其他包及加载器不会随依赖一起删除。

重新导入或在线更新也会保留既有配置。新包试图覆盖配置时会拒绝整批更新；新包省略的配置继续保留。

已声明的依赖按包身份和版本检查。启用时要求所需包已启用、版本匹配且记录的载荷完整；禁用时阻止仍被已启用包依赖的操作，删除时阻止仍被其他受管包依赖的操作。先处理提示中的使用方，再操作其依赖。

依赖声明来自包或来源元数据，不能证明任意第三方代码的全部运行要求。未提供完整声明的包会显示“未提供完整依赖声明”，需要结合作者说明检查加载器、客户端要求及运行兼容性。Thunderstore 的标准声明包含版本和依赖，但不包含发布团队身份，不能仅凭本地 `manifest.json` 推断完整的在线包身份。[包格式](https://wiki.thunderstore.io/mods/creating-a-package)。

BepInEx 所用的部分 Mono 文件接口仍受 Windows 长路径限制影响；实例程序目录应保持简短。服务器就绪本身不证明 Mod 已加载，应检查新的 `BepInEx/LogOutput.log`，预加载失败时检查程序根目录的 `preloader_*.log`。[加载器路径排障](https://docs.bepinex.dev/articles/user_guide/troubleshooting.html#extremely-long-paths-with-non-ascii-characters)。

管理操作持有实例和程序目录的变更锁。文件移动前持久化操作记录；中断后在下一次变更或启动前恢复，库存读取遇到未完成操作时停止显示并提示恢复。出现文件冲突或记录变化时停止恢复并保留证据，不覆盖新文件。恢复目录属于实例数据，应随实例程序目录保留，不应手工清理。

### 隔离原生验收

Windows 的显式验收测试 `isolated_valheim_thunderstore_file_mod_lifecycle` 使用只读的已安装 Valheim 源包，新建独立程序副本、实例、数据库和端口；拒绝管理员运行，不修改防火墙。先准备源包、官方 [Server_devcommands 1.115.0 ZIP](https://thunderstore.io/c/valheim/p/JereKuusela/Server_devcommands/1.115.0/) 和已存在的证据目录，全部位于仓库之外：

```powershell
$env:LGSM_FILE_MODS_NATIVE = 'true'
$env:LGSM_FILE_MODS_SOURCE_PACKAGE = 'C:\GamePackages\Valheim'
$env:LGSM_FILE_MODS_LOCAL_ARCHIVE = 'C:\ModPackages\JereKuusela-Server_devcommands-1.115.0.zip'
$env:LGSM_FILE_MODS_EVIDENCE_ROOT = 'C:\ModEvidence'
cargo test --locked -p langame-desktop --bin langame-desktop isolated_valheim_thunderstore_file_mod_lifecycle -- --ignored --test-threads=1 --nocapture
```

测试通过正式命令执行在线安装、禁用、重新启用、删除和本地重导入的五轮启动，核对新运行身份、实际就绪、插件加载及配置生成。依赖保护使用不含可执行文件的元数据使用方；不据此宣称另一个第三方依赖插件的玩法已验证。源包全树、配置、加载器和保留文件均核对 SHA；每轮日志及 JSON 报告写入新的证据子目录。已确认停服后才清理测试副本。

### 其他来源与运行环境边界

文件来源相同不代表加载方式相同。当前 12 个文件型工作流提供通用纳管；以下来源或运行环境仍有明确边界。此表说明功能覆盖，不能代替具体游戏、加载器和 Mod 版本的实际启动验收。

| 来源 / 游戏 | 当前可用范围 | 尚未接入的部分 |
| --- | --- | --- |
| mod.io / Core Keeper | 可导入兼容的原生包目录，每包目录包含 `ModManifest.json` | mod.io 在线下载和依赖解析；根目录直接包含清单的 ZIP 需先整理为独立包目录。[原生目录说明](https://github.com/CoreKeeperMods/Core-Keeper-Docs/blob/main/playing-with-mods/installing-mods/on-dedicated-servers.md) |
| Modrinth / Minecraft | 匹配已自行准备的运行环境的本地 JAR 纳管 | Fabric、Forge、NeoForge、Quilt 运行环境与版本绑定；自动 Modrinth 安装目前拒绝执行。Paper/Hangar/Spigot 的 `plugins` 流程也未接入。[Fabric](https://fabricmc.net/use/server/)、[Paper](https://docs.papermc.io/paper/adding-plugins/) |
| uMod/Oxide / Rust | 外部准备 Oxide 后，`oxide/plugins` 中本地 `.cs` 纳管 | 在线安装、加载器验证；Carbon 的独立目录和 `.cszip` 包不在此流程内。[uMod](https://umod.org/documentation/plugins/installation)、[Carbon](https://carbonmod.gg/devs/features/zip-script-packages) |
| SOTF Mods / Sons of the Forest | 外部准备 RedLoader 后，`Mods` 中本地 DLL 和独立目录纳管 | RedLoader 安装、跨 `Mods` / `Libs` 的包布局与依赖、网站下载。[安装说明](https://sotf-mods.com/install#dedicated-servers) |
| Necesse | 本地 JAR 和独立包目录纳管 | 在线下载及运行依赖识别。[Mod 说明](https://necessewiki.com/Modding) |
| FICSIT / Satisfactory | 查看目录与库存，使用 SMM 或 ficsit-cli 管理 | 通用导入、启停和删除暂时拒绝执行。`.smod` 不能原样复制；还缺 SML、服务器平台、`.uplugin` 目录和 `GameFeature` 路由的统一适配。[包布局](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html) |
| AML / Astroneer | 查看目录与库存，使用 AstroModLoader Classic 管理 | 通用导入、启停和删除暂时拒绝执行。每次清单变化都须重新生成 `999-AstroModIntegrator_P.pak`，只移动原始 `.pak` 无法证明禁用生效。[整合说明](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers) |
| Rocket / OpenMod / Unturned | 现有 Steam Workshop 资产流程 | 独立插件运行环境、插件目录和包管理。[Rocket](https://docs.smartlydressedgames.com/en/stable/servers/rocket.html)、[OpenMod](https://docs.smartlydressedgames.com/en/stable/servers/openmod.html) |
| TShock / Terraria | 现有原生和 tModLoader 流程 | TShock 入口、`ServerPlugins` 及独立配置 / 数据库。[TShock](https://github.com/Pryaxis/TShock/wiki) |

GitHub、作者网站和 ModDB 可以作为文件来源；使用当前支持格式的本地导入不等于接入了其网站 API，也不支持任意跨游戏根目录的安装包。

## English

Open **Mods → My Mods** for the instance. Imported local packages from Nexus Mods, Thunderstore, CurseForge and other sources share the file management controls. Files downloaded in a browser must first be imported. Game-managed Workshop and CurseForge IDs retain their existing enablement workflows; removing an instance entry does not purge shared download caches.

- **Manage files** records ownership for an identifiable existing Mod package. Official files, loaders, configuration directories and unrelated files are protected.
- **Disable / enable** requires a stopped server and moves a package outside its loading directory or restores it. Conflicting destinations are preserved. Disabled packages remain in My Mods.
- **Delete** checks dependencies and removes the package from the loading directory. Only unchanged recorded non-configuration files are deleted. Configuration, saves, additional files and modified files remain in the recovery location shown in the result. Dependencies and loaders are never removed automatically.

Reimports and online updates also retain existing configuration. Conflicting defaults reject the entire update; configuration omitted from the new package is retained.

Declared dependencies are checked by identity and version. Enabling requires matching enabled dependencies with intact recorded payloads. Disabling blocks enabled dependents; deletion blocks all managed dependents. Handle the named dependents before their requirements.

Package metadata cannot establish every runtime requirement of arbitrary third-party code. Packages without a complete declaration are labelled accordingly; consult the author for loader, client and compatibility requirements. The standard local Thunderstore manifest has no publishing-team identity, so it cannot establish the full online package identity. See the [official package format](https://wiki.thunderstore.io/mods/creating-a-package).

Some Mono file APIs used by BepInEx still have Windows path-length limits. Keep instance program paths short. Server readiness alone does not establish Mod loading: inspect a fresh `BepInEx/LogOutput.log`, or root-level `preloader_*.log` files when preloading fails. See the [loader troubleshooting guide](https://docs.bepinex.dev/articles/user_guide/troubleshooting.html#extremely-long-paths-with-non-ascii-characters).

Operations own the instance and program-directory leases. Durable intent precedes file moves; subsequent mutations and startup recover interrupted work. Inventory reads reject pending work without changing files. Conflicts or changed records stop recovery while preserving evidence. Keep recovery data with the instance program directory.

### Isolated native acceptance

The Windows opt-in test `isolated_valheim_thunderstore_file_mod_lifecycle` uses the four environment variables and portable command above. Supply a read-only installed Valheim source, the official pinned Server_devcommands 1.115.0 ZIP and an existing evidence directory, all outside the repository. The test refuses elevation and does not change firewall rules. It copies its own program, instance and database, selects independent ports, and performs five production start/stop cycles for online installation, disabling, re-enabling, deletion and local reimport. Fresh logs establish actual readiness, plugin loading and configuration initialization. Dependency refusals use a metadata-only consumer rather than another executable plugin. Whole-source and protected-file SHA checks, logs and a JSON report are retained outside the disposable fixture; fixture removal requires confirmed shutdown.

### Additional ecosystems and runtime boundaries

Twelve file workflows currently provide generic management. Local file support does not establish compatibility with every loader or distribution layout. Core Keeper's mod.io, Minecraft loader/version binding and Paper plugins, Rust Carbon, Unturned Rocket/OpenMod and Terraria TShock still require dedicated integration. Existing local routes cover Oxide scripts, RedLoader Mod directories and Necesse JARs after their runtimes are prepared externally. GitHub, author sites and ModDB are file sources rather than integrated download providers.

Astroneer and Satisfactory expose inventory and directory access while generic imports and mutations are blocked. Astroneer must regenerate its integrated package after every Mod-list change through [AstroModLoader Classic](https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers). Satisfactory needs SMM/ficsit-cli and the correct server-compatible `.uplugin` / `GameFeature` [package layout](https://docs.ficsit.app/satisfactory-modding/latest/ManualInstallDirections.html). Moving original files alone cannot establish correct enablement for these workflows.
