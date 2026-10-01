# Server Configuration / 服务器配置

[English](#english) | [简体中文](#简体中文)

## English

Open a server instance and select its configuration page. The available fields depend on the game module and its verified native settings.

### Find a setting

Use the sidebar search to find a setting by its name, native key, or category. Results show the full category path. Select a result, or press Enter for the first match, to open its category and focus the control. Press Escape or clear the search to return to categories.

| Category | Settings to look for |
| --- | --- |
| Room Settings | Server name, description, join password, player capacity, listing, region/language, and map or world selection |
| Administration & Permissions | Operator credentials, permission levels, command permissions, and admission policies |
| Network | Listen or advertised addresses, ports, and RCON enablement, credentials, and connection controls |
| Runtime & Advanced | Performance, process behavior, launch arguments, and advanced native overrides |
| Game-specific categories | World generation, game rules, progression, difficulty, and other native game settings |

Room Settings is the first category for all 32 games, including both ARK editions. Categories follow the setting's purpose, independently of the native file that stores it. World or map selection belongs to Room Settings; world generation, seeds, difficulty, PvP, and progression belong to game-specific world or gameplay categories. Only supported settings appear, so games need not have identical forms.

Detailed native groups remain where they help: ARK keeps its gameplay domains, Don't Starve Together keeps its cluster and shard rules, SCUM keeps its larger native setting groups, and Enshrouded keeps its role-specific permissions under Administration & Permissions. Host, performance, and raw override groups appear under Runtime & Advanced. Empty categories are hidden. Each setting has one editing location, without duplicate controls or relocation notices.

Command permissions, including 7 Days to Die's console-command permission levels, are configuration. The Tools tab performs immediate actions. Mod installation, enablement, load order, and supported Mod options belong to Mods. Individual player administrator assignments, access lists, and bans belong to Player Access where the game supports that workspace; role policies and operator credentials remain configuration.

Return to Moria's username-based permissions file is edited under **Player Access**. The editor reads the native file, including players added by the game; archived previews read the retained permissions without changing the archive. Existing-world DLC upgrades belong to **Maintenance → Existing-world DLC upgrade**, separate from DLC selection for a new world. Both editors require a stopped instance and an explicit save; changes apply on the next start. Upgrading an existing world cannot be undone, so back up the world first and check that participating players own the DLC.

SCUM's partial, gold and full wipe switches are under **Maintenance → Startup wipe switches**. Enabling a switch requires confirmation, followed by an explicit save while the instance is stopped. The native server performs the selected removal when started; saving the switches does not itself erase data. LanGame retains saved switches until you explicitly turn them off, so check them before every start. They are separate from automatic world-save and backup policies.

Immediate world saving belongs to **Maintenance → Saves and backups**, and broadcasts belong to **Maintenance → Broadcasts and automatic rules**. Minecraft, Terraria, Project Zomboid and Palworld no longer show a separate Tools page for these maintenance actions. Immediate saving asks the game server to persist the current world; creating a backup retains a recoverable copy.

Tools report command delivery separately from game effects and retain server responses, including rejection text. If console writing is still pending, check the runtime log before retrying; a batch stops at the first unconfirmed write or transport failure. RCON and REST tools require the corresponding instance settings and TCP port, followed by a server restart when those settings change. Project Zomboid uses its configured RCON port and password without a separate enable switch.

Both ARK editions use a server extension to spawn creatures and independently read them back by their native entity IDs. Stop the instance, install the extension from the local desktop Tools tab, then start the instance. Installation targets this instance's independent program directory and stops on unknown loaders or modified managed files. On first startup or after a game update, ASA sends the server EXE SHA-256 hash to the maintainer CDNs listed in the installation prompt to obtain matching symbol files; installation requires consent to those downloads. Enter world X/Y/Z coordinates in centimeters, rather than map latitude and longitude. Wild spawning does not require an online player; taming requires a real online character's numeric Player ID. Base level excludes bonus taming levels. Short class names resolve creatures already loaded by the map; unloaded or ambiguous Mod classes require a full `/Game/…_C` path and installed, enabled content. Results show the actual level, entity ID and position. Check the server before retrying an unconfirmed spawn or read-back.

ARK reward actions use the numeric in-game Player ID, not a Steam ID. DST player indices belong to the selected shard's `c_listallplayers()` output. Inventory delivery and revival address that shard explicitly; entities without an inventory-item component must be spawned nearby instead of placed in inventory. Precipitation can be snow in winter.

Player identities, access lists, bans, and administrator actions have separate support requirements. Check the [player capability matrix](player-center-capability-matrix.md) for the selected game.

### Save and apply changes

1. Edit the relevant fields and resolve any validation errors. Invalid configuration blocks automatic saving.
2. Allow automatic saving to finish. A saving error or conflict means the latest edit has not been accepted.
3. If saving fails, resolve the reported cause and use **Retry save**. For a conflict, preserve the intended changes, reopen the instance to load its current settings, and review the edit before submitting it again.
4. Check the game's requirements before applying changes to a running server. Saving a setting does not establish that the game has reloaded it; settings read only at startup require a server restart.

Save-game backups and configuration saves are separate operations. Before a change that affects an existing world or save, create a backup and check the game's documented restrictions.

### Automatic instance startup

Open an instance's **Maintenance** tab to enable **Start this server automatically when the app launches**. All 32 game modules share this control; each instance saves its own choice, disabled by default. Changes save immediately, with progress, confirmation, and retry on failure. This independent setting does not rewrite game configuration or backup policy.

The choice takes effect the next time LGSM's runtime service starts, including when changed while the instance is running. Reopening the interface while that service is still running does not repeat autostart. Enabled instances use the normal server-start workflow, with failures reported in background tasks. Windows startup of LGSM is an application-level setting.

### ARK SE and ASA configuration

Open **Cluster Transfers → Cluster maps** to add maps to one ARK instance. Each enabled map runs as its own native process with a separate LanGameCMD tab, registered ports, native log and world save, using the same installation, configuration and Mods. A blank Cluster ID is filled automatically when adding maps; the default transfer directory is shared within this instance. Stop the instance before adding, pausing or removing maps. Pausing or removing a map keeps its saved world; use a new map entry when changing its package. Start and Stop operate on all enabled maps, and ordinary world backups cover the complete native Saved tree, including retained map worlds.

Both editions retain LGSM's shared Room, Network, Administration, and Runtime categories. Attribute tables, player/creature experience curves, engram points, and native rule editors live in their gameplay categories. Switch to native text when needed; edits retain unknown Mod properties and unedited native values. Invalid numeric values, indexes, or tuple syntax block saving. Level CSV imports append new indexes without replacing existing curves. Class-name suggestions are a shared reference; availability still depends on the edition, map, and installed Mods.

The configuration toolbar imports `Game.ini` and `GameUserSettings.ini` with a preview, up to two files of 2 MiB each. Recognized entries populate their controls; unknown sections and repeated entries go into native overrides. Registered network parameters and the operational `ActiveMods` list remain managed by their existing workspaces. **Copy from instance** accepts another instance of the same edition and selected gameplay categories. Missing source overrides clear their target counterparts. Gameplay presets can be exported and imported without names, passwords, paths, network settings, cluster settings, or Mod installation lists.

Saving or regenerating ARK INI files preserves unmanaged entries and comments. Explicit native overrides take precedence; removing a previously managed override removes its native entry while retaining unrelated Mod settings. Pending edits made while the server runs are materialized when it next starts. These writes and their ownership records use the same conflict detection and rollback transaction.

Maps in one cluster need the same ARK edition, **Cluster ID**, and **Shared Cluster Directory**. An empty directory retains the instance's own cluster storage; matching IDs alone do not share uploaded data. Changing a path does not move existing uploads. Absolute local directories and UNC shares are supported. ASE and ASA must use separate transfer roots.

The cluster panel in **Maintenance** lists member maps, actual directories, and ports. It reports mismatched IDs or roots, shared roots used by another edition, overlapping directories, and members it cannot inspect. Refresh the group after changing membership. Group start and stop use each instance's normal lifecycle and report each member's result; a partial failure is not presented as whole-group success.

Cluster snapshots and restoration are available in the local desktop. First stop every member, refresh the member list, and confirm that the selected shared root belongs exclusively to this cluster. A snapshot includes every member's managed configuration directory, complete native `ShooterGame/Saved` tree, and the entire confirmed shared-transfer root. Another registered cluster cannot use the same root or a parent/child of it. A single-instance world backup does not provide this whole-group consistency.

Restore requires a separate confirmation of the snapshot and affected members. LGSM verifies membership and file checksums, creates a full protection snapshot of the current group, and stages all replacement data before publication. A publication error rolls back the group; if the process is interrupted or rollback cannot finish, the pending transaction blocks member startup. Use the explicit interrupted-restore recovery action: an uncommitted transaction rolls back to the protected state, while a fully committed transaction finishes cleanup. Configuration and lifecycle changes cannot run concurrently with the group transaction. Snapshots do not copy server executables or installed Mod packages.

Complete configuration restoration also requires the instance name, registered ports, bind address and autostart setting to match the snapshot. A mismatch is rejected before changing data; restore does not silently replace database registration with an older configuration mirror. Match those values before retrying.

**Maintenance → Crash recovery** exposes LGSM's existing recovery policy: enablement, consecutive restart limit, waiting time, and exit-code behavior. It applies while the runtime service is running, including after the interface closes. Explicit Stop cancels recovery. This is separate from scheduled maintenance and game or Mod updates.

### Instance isolation

Every instance owns its configuration, saves, logs and instance Mods. Minecraft vanilla supports a shared program installation, with an independent-install option when creating an instance. Other modules currently use independent installations. Every new independent instance copies program files; the downloaded library and excluded private data remain in their original location. Creation never starts a download. By default it verifies original program files; a missing or changed baseline requires an explicit library installation or repair. **Use existing local program** permits an independent instance to import program modifications without claiming official provenance. Declared configuration, save and Mod exclusions still apply. **Maintenance → Directories and diagnostics** shows verified paths, shared, private or damaged status, and conflicts with other registered instances.

During instance creation, Windows waits up to five seconds when copied program files are temporarily locked before publishing the runtime directory. Creation remains cancellable. Persistent file-access failures report the original OS error, and an existing destination is rejected.

Archive removes the active instance record and retains configuration, saves, Mods, logs and backups in the configured instance archive directory (initially `instances/.trash`). It can save space by omitting unchanged program files only when a verified library contains the exact originals; modified and unknown files remain in the archive. If that proof is unavailable, the complete program is retained and the reason is shown. External saves remain in place and are backed up before archiving. Delete is separate: it permanently removes the entire managed instance directory, including its local saves and backups, without creating a recoverable archive. Both operations preserve the downloaded library, other instances and external saves.

On the host, use the **Instances / Archives** switch beside the instance search in **Servers**. Archived cards use **Restore** in place of **Start**. Selecting a completed archive opens the same detail tabs as an instance: Runtime, Configuration, Mods, Players, Maintenance and Tools. Runtime is selected first, with the same navigation layout as an active instance. The archive reuses the normal Runtime, Configuration, Mods, Players and Maintenance components with saved instance data and a read-only mode. Retained logs appear in LanGameCMD; saved configuration and access rules, Mod references, maintenance policies and backup metadata use the same controls and navigation. No separate archived detail page renderers are maintained. Game capabilities still determine which tabs are available; server tools require restoration. Archived details do not query online players or execute server commands. Restore the instance before editing. Archive dates, file locations and recovery requirements remain in the card tooltip. Port conflicts or unavailable reconstruction files can prevent restoration without preventing detail preview. Permanent archive cleanup remains a confirmed card action. Incomplete instance deletions appear as **Deletion failed** cards in the Instances list, with the original error and an explicit retry action. These lists do not require a storage usage scan.

**System → Storage & runtime → Instance archives** provides directory selection and opening, alongside the instance workspace and server-file directories. A new archive directory must be empty, separate from managed data, and on the same volume as the instance workspace. Finish retained archives and failed deletions before changing the instance or archive root; changing the setting does not move existing files.

Restoration returns the original instance ID, directory, ports, configuration, program ownership, and history. If program files were omitted, restoration requires an existing verified library with the recorded package fingerprint; a missing or different package requires explicit installation or repair, never an automatic download. External saves are not overwritten: the retained backup can be restored separately after review. Restoration refuses conflicting paths, IDs, ports, changed ownership, or incomplete metadata. The restored instance stays stopped with autostart disabled. Historical archives without recovery metadata remain available for manual file recovery or permanent cleanup. Permanent cleanup requires confirmation and validates directory identity, references, and the complete tree before deleting files. Failed deletion remains visible for inspection and retry; startup never resumes permanent deletion automatically. These host maintenance actions are unavailable over the LAN web interface.

Creation, configuration saves, startup materialization, and backup restoration check configuration and save ownership, including Windows case aliases, ancestor overlap, and directory junctions. Conflicts block writes. An explicitly configured ARK cluster-transfer directory may still be shared; it is separate from world-save ownership.

Missing runtime directories, invalid ownership markers, and unsafe paths fail explicitly. Shared program updates require all referencing instances to be stopped, including pending starts. Independent installations update explicitly from instance maintenance and do not inherit library updates at startup; full-directory replacement installers are refused in this path to preserve instance data. Mod operations that change shared program files first detach that instance to an independent installation. Recovery consults committed ownership before restoring an interrupted transfer or detachment. A complete private runtime remains launchable when the library package is unavailable. Regular backups and pre-restore safeguards remain available; failed isolation checks never rebuild or delete data. Directory isolation is not equivalent to container or operating-system account isolation.

The same isolation applies when running different games together. Port allocation also respects native groups: ASE's UDP peer port is always its game port plus one, so creation, network edits, and startup conflict remapping keep the pair together. A conflict on either port moves the whole pair. Other declared groups, including Valheim and Core Keeper, retain their own relationships. This does not imply that ASA uses ASE's port contract.

### Instance resource limits

Stop the instance before editing resource limits in **Maintenance**. Save the policy and start the instance again to apply it. An empty CPU or memory limit leaves that limit disabled; the display distinguishes saved settings from limits already applied to a running instance.

CPU is a percentage of the whole host's CPU allocation, not one logical processor. Memory is the Windows Job's total committed-memory cap, in MiB (1 MiB = 1,048,576 bytes), rather than a working-set target. The limits cover every owned process in the instance; DST Master, Caves and their descendants share one budget. Set the memory cap for the complete instance, not separately for each shard.

When a memory budget is configured, startup checks configured active budgets against host capacity and pending launches against currently available memory, retaining the configured host reserve. Insufficient capacity rejects the launch before starting its processes. CPU caps and memory budgets do not provide a separate disk, network connection, or security account.

### Closing the interface and stopping the runtime

On Windows, closing the main window hides it to the system tray while the current user's local runtime service and managed servers keep running. Click the tray icon or open LanGame again to restore the interface. The tray menu follows the interface language and contains **Open** and **Exit**. **Exit** immediately stops interface refreshes, hands shutdown ownership to an independent helper, and closes the interface and tray without waiting for game saves. The backend stops servers through the same save/stop workflow as manual stopping; it creates backup archives only when the instance's automatic backup on stop setting is enabled. No additional save is dispatched just for exit. The backend ends after cleanup, with a 120-second final deadline from the first click if saving or cleanup stalls; unsaved data may be lost at that cutoff. Repeated clicks do not extend the deadline. Closing the window alone does not stop servers.

Application updates download their package before requesting runtime shutdown, and install only after shutdown succeeds. The runtime is a process in the signed-in user's session, not an installed Windows system service. Continued operation across sign-out or Windows restart is not guaranteed.

### Don't Starve Together worlds and safe lifecycle

Each instance owns a separate `config/clusters/main` cluster, with `Master` and optional `Caves` shards. The official dedicated-server executable generates a new world on its first launch from the configured world-generation rules, or loads the existing native save. Generating a world in the game client first is optional. Editing generation rules does not regenerate an existing world.

Every DST instance has a private `runtime` program directory. Saves and configuration belong to the instance; each shard has its own `modoverrides.lua` enablement/options and `data/ugc/Master` or `data/ugc/Caves` Mod files. The download list serves both shards within this instance. Across instances, Mod installation shares only the reusable machine download cache, not enabled lists or option values. Missing or invalid runtime directories block operations. Detail reconciliation, launch preview, and startup can restore a validated rollback directory left by an interrupted update while the instance is stopped; they do not rebuild a missing runtime from shared files.

Creating an instance opens its settings without generating a map. Set the complete Surface and Caves **World generation** parameters there, then select **Start**. Startup waits for pending configuration saves; invalid settings, save failures and conflicts prevent launch. LGSM identifies existing saves and verifies that the configuration and save state remain unchanged before launching. The official server then generates new worlds or loads existing saves directly. If the configuration or save state changes during startup preparation, retry the start. Unrecognized save data must be checked before startup. Custom Lua scripts determine their own effective generation parameters and can be reviewed under Advanced settings.

To import an existing world, stop the instance and open **Maintenance → Import an existing world**, next to its backups. Configuration retains the world-generation parameters; importing a save is a separate maintenance operation. World import requires every enabled shard and a non-empty snapshot/.meta pair in the session referenced by its native `shardindex`. Additional shard directories are rejected because this launcher manages Master and Caves. LGSM validates the source, creates a backup available in **Backups**, stages the complete replacement, and restores the previous cluster if publishing fails. Instance configuration and Mod settings are preserved. A missing disabled Caves shard clears that shard's old world so it cannot later be mixed with an imported surface world. Source and destination must not overlap.

Startup checks online token requirements, mode consistency and LAN player ports (`10998`–`11018`). Draft settings can be saved before an online token is entered. A server becomes ready only after each enabled world is initialized and Master confirms the Caves connection. Native configuration errors and unknown presets fail startup. LanGameCMD shows both shards' logs during generation. Five minutes without recognized generation progress, or fifteen minutes overall, ends the wait with the relevant shard logs. Per-shard Workshop caches live under `data/ugc`, outside world backups and other instances' caches.

If one shard crashes, the remaining shard stays visible and controllable, with its log stream intact. **Stop** asks every surviving shard to save and shut down, waits up to 90 seconds for native save completion and process exit, and retains the real exit code. Missing confirmation or an abnormal exit reports failure and skips the automatic backup. Surviving processes stay managed; they are not silently killed after an unconfirmed save.

When automatic restart is enabled, a shard crash first saves and stops surviving shards, then restarts the complete instance from its saves. Unconfirmed saves prevent recovery. The restart limit counts failed sessions independently of the eight entries displayed in history. Explicit Stop cancels pending recovery; unexpected zero-code exits follow the configured exit policy.

The native layout and launch parameters follow Klei's [dedicated-server setup](https://kleiforums.com/forums/topic/64212-dedicated-server-quick-setup-guide-windows/) and [command-line reference](https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide).

Shared world settings, including seasonal event switches, inherit from Master into Caves following Klei's world creation rules. Explicit advanced Caves overrides take precedence.

### Mod coverage across all 32 games

The server catalog contains 32 games; Mod support depends on each module's actual installation and loading contract. This is the complete current coverage list:

| Workflow | Count | Games |
|---|---:|---|
| Server Steam Workshop and instance collections | 10 | Don't Starve Together, Project Zomboid, Unturned, ARK: Survival Evolved, Barotrauma, Conan Exiles, Palworld, Squad, Terraria/tModLoader, Soulmask |
| Verified Thunderstore package links and local file import | 3 | Core Keeper, Valheim, V Rising |
| CurseForge project IDs and local file import | 1 | ARK: Survival Ascended |
| Local file import with source links | 11 | Abiotic Factor, Astroneer, Enshrouded, HumanitZ, Minecraft, Necesse, Rust, Satisfactory, 7 Days to Die, Sons of the Forest, Windrose |
| Client dependency explanation only | 1 | RimWorld Together |
| No LanGame server Mod workspace | 6 | Nightingale, Return to Moria, Romestead, RuneScape: Dragonwilds, SCUM, The Forest |

The 14 file-based games in the Thunderstore and local-import groups expose instance inventory and **Open Mod folder**, but currently have no generic enable/disable or removal action. Importing a package does not install its required loader or prove that the game loaded it. Thunderstore links require runtime and dependency verification; Minecraft accepts matching local JAR files, while automatic Modrinth installation is blocked without an instance loader/version contract. ASA manages its CurseForge ID list separately. Non-Steam sources do not use **My collections**. A missing LanGame workflow does not imply that the game has no external mod ecosystem.

The manifest's `supports_collections` flag does not control the current UI. LanGame expands collections into verified server packages for the ten Steam workflows above; a false flag must not be interpreted as disabling that app-managed capability.

ASA's **My Mods** includes both active and passive loading IDs. Disabling an entry removes it from both loading lists while retaining its original loading modes for re-enablement, even before files have been downloaded. Removing it clears instance membership and hides retained cache files; explicitly adding its ID or importing that package again restores it. Local import restores only the roots written by that import and does not automatically enable them. If custom launch flags contain `-mods` or `-passivemods`, management controls are blocked until those flags are removed, so a raw option cannot silently keep a disabled Mod loaded. Both loading lists normalize multiple IDs into one comma-separated argument.

For a read-only declaration check covering every shipped module, run `python -B scripts/audit_mod_workflows.py --check`. Its result describes code coverage, not a successful launch of all 32 game servers.

### Workshop and Don't Starve Together Mods

The **Mods** tab searches the selected game's Steam Workshop. Typing a name selects relevance ordering; you can choose popularity over the past week, all-time rating, newest, or most subscribed. Searches use the interface language and also accept a Workshop item ID or full item URL. Page loading and failures are shown explicitly; a failed request retains the last successful page and offers retry.

For Don't Starve Together, **Install** downloads the Workshop files through SteamCMD before saving this instance's download list and shard enablement. Select a Mod under **My Mods** to read its local `modinfo.lua` configuration. Missing files, read failures, and a Mod declaring no options are separate states. All ordinary Mod management is in this workspace. Its scope selector applies options to both shards or independently to Master/Caves; advanced raw Lua overrides remain in Configuration. Text and numeric options save when you leave the field or press Enter. Reading options does not rewrite instance configuration, and running instances remain read-only.

Start waits for pending Mod changes, including preliminary reads and downloads. Failed changes block launch until retried or the persisted settings are reopened. A shard using custom raw `modoverrides.lua` cannot also use a structured enabled list or options. Nested collections must be expanded through Manifest mode; ordinary installation does not silently install only part of a collection.

Disabled DST Mods remain in **My Mods** with their saved options. Re-enabling an existing instance entry changes its configuration without requiring an online lookup or another download. Editing both shards warns when their effective values differ, including when one shard uses the default.

Installation deploys the selected complete Workshop packages and their official installation records into this instance's independent Master and Caves caches, including when the shared download cache is reused. Incomplete caches are downloaded again. Download timeouts and native Mod load errors fail startup and identify the shard log; world initialization alone does not confirm Mod loading.

Configured entries are checked against local files, including DST's per-shard UGC downloads. An enablement checkbox records the requested configuration; missing files are reported separately. Steam guides cannot be installed as Mods. Client-only DST Mods are identified separately and configured in the game client. Existing invalid entries remain available for removal from the instance.

### Workshop ID lists

Workshop subscriptions and enabled Mod lists belong to the **Mods** tab, including Barotrauma, Conan Exiles, Soulmask, and Project Zomboid. Project Zomboid's map load order is also owned by Mods, which handles map installation, ordering, and removal. Configuration does not duplicate these controls.

**Manifest mode**, after **My Mods** in the top toolbar, accepts Workshop IDs, item URLs, and collections separated by lines, commas, or semicolons. It deduplicates entries, expands collections in input order, and checks game ownership, package type, and existing files before enabling actions. **Download missing** keeps enablement unchanged; **Download missing and enable** merges into existing settings. Applying a list checks disk again, reuses cached packages, and deploys them into the same instance directories used by the game. Updating a selected package replaces its old contents so removed upstream files do not remain active; unrelated packages and configured Mods are preserved. Lists support up to 8,192 entries and 1 MiB of text.

Shared Workshop cache reuse requires an installed record for the correct game, a non-empty payload matching its recorded byte count, and no conflicting newer manifest. Empty, unrecorded or size-mismatched directories are downloaded again; this check does not authenticate individual file contents.

The **Mods / Collections** switch also applies to the instance library: **My Mods** becomes **My collections** in collection mode. Each added collection expands to its members; selecting an added Mod opens its settings, and **Add missing Mods** restores missing membership. Successful collection installation through either browsing or Manifest mode saves the collection ID, title, and member snapshot with the instance settings. Collections remain visible offline and after restart. Existing DST collection download IDs remain visible even without a saved member snapshot; other historical installations cannot be inferred from matching Mod IDs alone.

Collection members and **My Mods** control the same instance entries. DST, Project Zomboid, Palworld, ARK: Survival Evolved, Barotrauma, Conan Exiles, and Soulmask support individual and collection-wide enablement; disabled entries remain owned and can be enabled again without downloading. A mixed checkbox means only some members or internal packages are enabled. Project Zomboid reads fresh internal Mod IDs and map metadata before changing them and retains names still needed by other installed Workshop items. Removing a Palworld member also records its removed membership, so retained package files do not make it reappear in the library. Squad supports individual instance removal while preserving the collection snapshot for repair; it has no independent enablement switch. Unturned exposes its download list, and tModLoader still requires internal Mod names for enablement; neither receives a Workshop-ID enablement switch. When tModLoader names are enabled but their Workshop mapping is unavailable, disable those names in settings before removing member entries.

Collection removal opens a member review. **Remove collection and selected Mods** removes the collection record and selected members from the current instance; other collections' shared members are protected, and members can be unchecked to keep them. Downloaded files and saved Mod options are retained. Squad members move outside its plugin loading directory into instance-local retained storage with recovery for interrupted operations; shared Workshop caches are untouched. **Remove collection record only** preserves every member. Missing collection snapshots prevent automatic shared-member checks, and incomplete local metadata prevents guessing internal Mod names. In particular, enabled tModLoader names must first be disabled in settings when their Workshop mapping is unavailable. Changed instance settings require reopening the review before removal.

Thunderstore installations use stable provider/project identities and an ownership record. Reinstalling or updating replaces the same package, preserves additional user files, and stops on conflicting edits. An untracked copy from an earlier installation is reported with its path for review before a duplicate can be installed.

Conan and Barotrauma also load manually imported packages whose names start with the Workshop ID. Missing or ambiguous payloads prevent publishing an incomplete native list. Conan rejects conflicting PAK filenames and preserves untracked or externally modified native files. Barotrauma updates the actual instance's `config_player.xml`, preserving other settings and its core package; the runtime copy is used only to initialize a missing instance file.

The native destination is shown in the list editor. DST, Unturned, and Soulmask consume their native lists at server startup. ARK SE enablement synchronizes `ActiveMods`, `ModInstaller.ModIDS`, and automatic installation. PZ reads local Mod IDs and map metadata; Palworld reads `Info.json` PackageName. Conan and Barotrauma generate their package loading files from the deployed payload. Squad installs plugins without claiming a separate ID enablement setting. For tModLoader, downloading saves the `install.txt` input but preserves existing `enabled.json` names; internal Mod names and the tModLoader runtime must be configured separately, as described in the [official dedicated-server documentation](https://docs.tmodloader.net/docs/stable/md__github_workspace_src_t_mod_loader__terraria_release_extras__dedicated_server_utils__r_e_a_d_m_e.html).

### Report an absent or duplicate setting

Include the game module, the configuration category, the native key or startup argument, and a publisher or dedicated-server reference. State the expected behavior and any duplicate location. Remove passwords, access tokens, player data, and private addresses from examples.

The [configuration source ledger](game-config-source-ledger.md) and [acceptance records](game-config-acceptance/) distinguish implemented settings from documented exclusions and unverified native behavior. They describe evidence coverage; they do not guarantee support for every setting added in a later game update.

## 简体中文

打开服务器实例，进入配置页。可编辑字段取决于对应游戏模块及已经验证的原生设置。

### 查找设置

可在侧栏按设置名称、原生键名或分类搜索。结果显示完整分类路径；选择结果或按 Enter 打开首个结果时，会切换分类并聚焦对应控件。按 Escape 或清除搜索可返回分类导航。

| 分类 | 设置内容 |
| --- | --- |
| 房间配置 | 服务器名称、描述、加入密码、人数、列表展示、地区/语言及地图或世界选择 |
| 管理权限 | 管理凭据、权限等级、命令权限与准入策略 |
| 网络 | 监听或公开地址、端口，以及 RCON 开关、凭据与连接控制 |
| 运行与高级 | 性能、进程行为、启动参数与高级原生覆盖 |
| 游戏专属分类 | 世界生成、游戏规则、成长、难度及其他原生游戏设置 |

全部 32 个游戏均以房间配置为第一项，包括两款 ARK。分类按设置用途划分，不由原生文件结构决定。地图或世界选择放在房间配置；世界生成、种子、难度、PvP 和成长倍率放在游戏专属的世界或玩法分类。仅显示游戏支持的设置，各游戏不必拥有相同字段。

有意义的原生分组继续保留：ARK 的玩法领域、饥荒的集群与分片规则、SCUM 较大的原生设置分组，以及雾锁王国位于管理权限下的角色权限。主机、性能和原生覆盖分组位于运行与高级下。空分类不显示；同一设置只保留一个编辑入口，不重复显示控件或迁出说明。

命令权限属于配置，包括七日杀的控制台命令权限等级。工具选项卡承载即时操作。模组的安装、启用、加载顺序及受支持的模组选项位于模组。游戏支持玩家访问工作区时，具体玩家的管理员身份、访问名单和封禁在那里管理；角色策略与管理凭据仍属于配置。

重返莫瑞亚按用户名管理的权限文件位于「玩家管理」。编辑器读取原生文件，保留游戏运行时新增的玩家；归档预览读取归档内保留的名单，不修改归档内容。旧世界 DLC 升级位于「维护 → 旧世界 DLC 升级」，与新世界创建时的 DLC 选择分开。两处编辑器均要求先停服并显式保存，下次启动时生效。旧世界升级无法撤销，操作前应备份世界，并确认参与玩家拥有对应 DLC。

SCUM 的部分擦除、金币擦除和完整擦除开关位于「维护 → 启动擦除开关」。启用开关时需要确认，停服后再显式保存。原生服务器启动时执行所选擦除；保存开关本身不会删除数据。LanGame 会持续保留已保存的开关，需手动关闭不再需要的项，因此每次启动前都应检查。这些选项与自动世界存档、备份策略分开管理。

即时保存世界位于「维护 → 存档与备份」，广播位于「维护 → 广播与自动规则」。Minecraft、泰拉瑞亚、僵尸毁灭工程和幻兽帕鲁不再为这些维护动作重复显示工具页。立即保存请求游戏服务器写入当前世界，备份则保留可恢复副本，两者用途不同。

工具页区分命令送达与游戏内生效，并保留服务器原始响应，包括拒绝信息。控制台写入仍待确认时，请先检查运行日志再重试；批次在首个未确认写入或传输失败处停止。RCON、REST 操作需要配置对应的实例开关和 TCP 端口，修改后重启服务器；僵尸毁灭工程使用已配置的 RCON 端口与密码，没有额外启用开关。

方舟两版的「生成生物」使用服务端扩展直接生成并按生物 ID 读回确认。首次使用先停服，在本地桌面工具页安装扩展，再启动实例。安装仅写入本实例独立程序目录，遇到未知加载器或被修改的受管文件会停止。ASA 扩展在首次启动或游戏更新后，需要向页面列明的维护者 CDN 发送服务器 EXE 的 SHA-256 哈希以下载匹配文件；安装前会说明并取得允许。填写游戏世界 X/Y/Z 坐标（厘米），不能使用地图经纬度；野生生物无需在线玩家，驯服生物需要填写真实在线角色的数字 Player ID。基础等级不额外叠加驯服奖励。目录短类名用于当前地图已加载的生物，未加载的或有歧义的 Mod 类需要完整 `/Game/…_C` 路径及已安装启用的内容。生成结果包含真实等级、生物 ID 和坐标；回执或读回不明时，先检查服务器，避免重复生成。

ARK 发放物品使用游戏角色的数字 Player ID，不是 Steam ID。饥荒玩家序号以所选分片的 `c_listallplayers()` 输出为准，发背包和复活均明确针对该分片；不能装入背包的实体应选择在玩家身边生成。降水在冬季可能表现为下雪。

玩家身份、访问名单、封禁和管理员操作有各自的支持条件，详见对应游戏的 [玩家能力矩阵](player-center-capability-matrix.md)。

### 保存与生效

1. 修改相关字段，并处理校验错误。配置无效时，自动保存会被阻止。
2. 等待自动保存完成。出现保存错误或冲突时，表示最新改动尚未被接受。
3. 保存失败时，处理错误原因后选择「重试保存」。出现冲突时，先保留准备修改的内容，再重新打开实例以加载当前设置，核对后重新修改。
4. 修改运行中的服务器前，确认游戏的生效要求。保存设置不代表游戏已重新加载配置；只在启动时读取的设置需要重启服务器。

存档备份与配置保存是不同操作。修改可能影响现有世界或存档的设置前，应先创建备份，并查阅游戏文档中的限制。

### 实例自动启动

在实例的「维护」页启用「应用启动时自动启动该服务器」。32 款游戏共用这一入口，每个实例独立保存，默认关闭。切换后立即保存，显示保存进度和结果，失败时可重试。此设置独立保存，不会重写游戏配置或备份策略。

设置在下次启动 LGSM 运行服务时生效，实例运行中也可修改。运行服务仍在后台时，重新打开界面不会再次执行自启动。已启用的实例通过正常服务器启动流程开服，并在后台任务中记录失败。Windows 开机启动 LGSM 属于应用级设置。

### 多实例隔离

每个实例独立保存配置、存档、日志和实例 Mod。Minecraft 原版支持共享程序，创建时也可选择独立安装；其他模块当前使用独立安装。每个新独立实例复制程序文件，已下载的程序库及被排除的私有数据保留在原处。创建不会自动下载；默认核验原版程序，清单缺失或文件被修改时，需单独安装或修复程序。明确选择「使用现有本地程序」可将程序修改导入独立实例，但不认证为官方原版，模块声明的配置、存档及 Mod 排除规则仍然生效。「维护 → 目录与诊断」显示后端核实的实际路径、共享、独立或损坏状态，以及与其他实例的路径冲突。

创建实例时，如果刚复制的程序文件被临时占用，Windows 最多等待 5 秒再完成运行目录发布，等待期间仍可取消。文件访问持续失败时会报告原始系统错误，已存在的目标目录会被拒绝覆盖。

归档会移除活动实例记录，将配置、存档、Mod、日志和备份保留在指定的实例归档区，初始位置为 `instances/.trash`。只有可信程序库包含完全相同的原版文件时，归档才省去未修改的程序文件以节省空间；修改过或无法识别的文件继续保留。无法证明可还原时，保留完整程序并显示原因。外部存档保留在原处，归档前另做备份。删除是独立操作：永久移除整个托管实例目录，包括其中的存档和备份，不生成可恢复归档。两项操作都保留已下载的程序库、其他实例及外部存档。

在主机的「服务器」页，实例搜索框旁可切换「实例／归档」。归档卡片的主操作为「还原」。选择完整归档后，右侧保留与普通实例相同的「运行、配置、模组、玩家、维护、工具」选项卡，默认选中第一个「运行」，运行、配置、模组、玩家和维护直接复用普通实例的页面组件，以归档数据进入只读模式。保留日志显示在原 LanGameCMD 控制台，配置、访问规则、模组引用、维护策略及备份信息沿用原控件和分类导航，不再维护独立的归档详情页面。各游戏的支持能力仍决定选项卡是否可用，服务器工具须先还原实例。归档详情不查询在线玩家或执行服务器命令，还原实例后才能编辑。归档时间、文件位置及恢复条件保留在卡片气泡里。端口冲突或重建所需程序文件不可用可能阻止还原，但不影响详情预览。永久清理仍需在卡片内确认。未完成的实例删除作为「删除失败」卡片留在实例列表，显示原始错误并提供明确的重试操作。查看这些列表无需运行空间占用扫描。

「系统 → 存储与运行环境 → 实例归档区」与实例工作区、服务器文件并列，提供指定目录与打开文件夹功能。新归档目录须为空、与其他托管数据分离，并与实例工作区位于同一卷。更改实例或归档根目录前，应先处理现有归档及失败删除；修改设置不会迁移已有文件。

恢复会还原原实例 ID、目录、端口、配置、程序归属及历史。若归档省去了程序文件，需要现有可信程序库与记录的程序指纹匹配；缺少或版本不符时，需明确安装或修复，不会自动下载。外部存档不会被覆盖，确认后可单独恢复保留的备份。路径、ID、端口冲突、归属变化或元数据不完整时拒绝覆盖。恢复后保持停服并关闭自动启动。缺少恢复元数据的历史归档仍可自行取回文件或永久清理。永久清理需确认，并在删除前核验目录身份、引用和整个目录树；删除失败会保留状态供检查和重试，启动时不会自动继续永久删除。上述主机维护操作不向局域网网页入口开放。

创建、保存配置、启动前生成配置和恢复备份时，后端检查配置与存档归属，包括 Windows 大小写、目录包含关系和目录联接。检测到跨实例重叠时拒绝写入。ARK 显式设置的集群传输目录仍可共享，它不属于世界存档目录。

运行目录缺失、所有权标记无效或路径异常时，操作会明确失败。共享程序更新须先停止所有引用它的实例，并取消待启动操作。独立安装从实例维护入口显式更新，启动时不继承游戏库更新；整目录替换型安装器在此入口被拒绝，以保留实例数据。修改共享程序文件的 Mod 操作会先将该实例拆分为独立安装。接管或拆分中断后，恢复流程按数据库已提交的权属选择正确目录。游戏库安装包暂不可用时，完整的独立实例仍可使用自身程序启动。普通备份及恢复前保障备份继续保留，数据不会因隔离检查失败而被重建或删除。独立目录提供数据隔离，不等同于容器或不同操作系统账号的权限隔离。

不同游戏同时运行也使用相同的隔离机制。端口分配同时遵守原生端口组：ASE 的 UDP peer 端口始终等于游戏端口加一，创建、修改网络设置以及启动时处理占用冲突都会保持整组关系，任一端口冲突时整组移动。Valheim、Core Keeper 等已有端口组也保留各自关系；这不代表 ASA 使用 ASE 的端口契约。

### 实例资源限制

在「维护」中修改资源限制前先停止实例，保存后下次启动生效。CPU 或内存上限留空即不启用对应限制；界面区分已保存配置与正在运行的实例实际采用的限制。

CPU 百分比以整台主机的 CPU 配额为基准，不是单个逻辑处理器。内存是 Windows Job 的提交内存总上限，单位 MiB（1 MiB = 1,048,576 字节），不是工作集目标。限制覆盖实例拥有的全部进程，饥荒的地表、洞穴及后代进程共享一个预算，应按完整实例设置内存，而非分别为每个分片填同一份额度。

启用内存预算后，启动前会检查活动实例的配置预算与主机容量，并根据当前可用内存检查待启动预算，同时保留设置的主机余量。容量不足时在创建进程前拒绝启动。CPU 和内存限制不提供独立磁盘、网络连接或安全账号。

### 关闭界面与停止运行服务

在 Windows 上关闭主窗口会隐藏到系统托盘，当前用户的本地运行服务及受管服务器继续运行。点击托盘图标或再次打开 LanGame 即可恢复界面。托盘菜单跟随界面语言，中文显示「打开」和「退出」。「退出」会立即停止界面刷新，将停服责任交给独立助手，随后关闭界面和托盘，不等待游戏保存。后台沿用手动停止的保存、停服流程；只有实例开启「停止时自动备份」才创建备份归档，不因退出额外发送保存命令。后台清理完成后自行结束；若保存或清理卡住，自首次点击起最多等待 120 秒后强制结束，届时尚未保存的数据可能丢失。重复点击不会延长期限；单纯关闭窗口不停止服务器。

应用更新先下载更新包，再请求停止运行服务，确认停服成功后才安装。运行服务是登录用户会话内的进程，不是已安装的 Windows 系统服务，不保证在注销或 Windows 重启后继续运行。

### 饥荒世界与开停服

每个实例有独立的 `config/clusters/main` 集群，包含地表 `Master` 与可选的洞穴 `Caves`。首次启动时，由官方专用服务器程序按世界生成配置创建世界；已有原生存档时则加载存档。无需先在游戏客户端生成世界。修改生成规则不会重新生成已有世界。

每个饥荒实例都拥有独立的 `runtime` 程序目录。存档和配置归属当前实例；两分片各自使用 `modoverrides.lua` 保存启用列表和选项，模组文件分别位于 `data/ugc/Master`、`data/ugc/Caves`。下载列表供此实例的两分片使用。跨实例安装模组时，共享的只是可复用的机器下载缓存，不共享启用列表和选项值。运行目录缺失或无效时会阻止操作。实例停止时，详情协调、启动预览和启动入口可以恢复更新中断留下且通过校验的回滚目录；不会用共享文件自动重建丢失的运行目录。

地表生成、地表设置、洞穴生成和洞穴设置使用 `modules/dontstarve/world-options.json` 中的官方分类与顺序，不根据字段名推测归属。清单由官方 PC 专用服务器脚本提取，记录游戏版本和脚本包哈希；显示范围遵循地表、洞穴及主世界专属规则。组内优先使用官方指定顺序，其余按当前界面的本地化名称排序。未知配置保留在「其他设置」，不会隐藏或误归入生物。Mod 自行注册的世界选项不属于这份原版清单，仍需通过高级 Lua 配置。

开发验证入口 `verify:module-settings` 会逐项检查原版清单与 schema 的四页字段和可选值。官方程序更新后，在仓库根目录运行 `python -B scripts/verify_dst_world_options.py --install-root "<DST 安装目录>"`，直接比对实际包中的字段、分类、顺序和档位；漂移会返回失败。完成差异审查后可加 `--write` 刷新清单，再同步 schema、翻译和测试。工具只读解析 Lua 数据，不执行游戏或 Mod 脚本，不会自动更改实例配置。

创建实例后会进入设置页，此时尚未生成地图。在此完整设置地表与洞穴的「世界生成」参数，然后点击「启动」。启动会等待尚未完成的配置保存；无效配置、保存失败或冲突会阻止启动。LGSM 在后台识别已有存档，并在启动锁内核验配置与存档状态未发生变化，随后由官方服务器程序直接生成新世界或加载已有存档。启动准备期间发现配置或存档状态变化时，重试即可；无法识别的存档数据需先检查。自定义 Lua 脚本的实际生成参数以脚本为准，可在高级设置中查看。

导入已有存档时，先停止实例，再打开「维护」页备份区域的「导入现有世界」。配置页保留世界生成参数，存档导入作为独立维护操作执行。导入必须包含所有已启用的分片，并在原生 `shardindex` 指向的 session 中找到非空快照及其 `.meta` 文件。当前管理地表和洞穴两片，含额外分片目录的存档会拒绝导入。LGSM 先验证源存档，创建可在「备份」中恢复的备份，再暂存完整替换内容；发布失败时恢复原集群。实例配置和模组设置保留。洞穴未启用且源存档不含洞穴时，会清除旧洞穴世界，避免以后与新地表混用。源目录与目标目录不能相互包含。

开服前检查在线令牌、模式组合和局域网玩家端口范围（`10998`–`11018`）；令牌尚未填写时仍可保存草稿配置。所有已启用世界完成初始化，且地表确认洞穴连接后，才判定开服成功。原生配置错误或不存在的预设会使启动失败。生成期间，LanGameCMD 同时显示两片日志；连续五分钟没有可识别的生成进展，或总计超过十五分钟时结束等待并提示对应日志。各分片的创意工坊缓存放在 `data/ugc`，与世界备份及其他实例隔离。

某个分片崩溃后，存活分片仍可查看日志和控制。「停止」会请求每个存活分片保存并关闭，最多等待 90 秒确认原生存档完成与进程退出，并保留真实退出码。未确认保存或退出异常时会报错并跳过自动备份；存活进程继续受管，不会在保存未确认时被静默强杀。

启用自动重启后，分片崩溃会先保存并停止存活分片，再整组读档重启；未确认保存时不会继续恢复。重启上限按失败会话计数，不受界面仅展示八条历史的限制。主动停止会取消待执行的恢复；非主动、退出码为零的退出是否重启，遵循配置的退出策略。

原生目录和启动参数依据 Klei 的[专用服务器配置指南](https://kleiforums.com/forums/topic/64212-dedicated-server-quick-setup-guide-windows/)与[命令行参数文档](https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide)。

活动开关等共享世界规则按 Klei 的建档规则从地表继承到洞穴；洞穴高级配置中的显式覆盖仍优先。

### 全部 32 款游戏的 Mod 覆盖范围

服务器目录共有 32 款游戏，Mod 能力以各模块实际接入的安装与加载方式为准。当前完整清单如下：

| 接入方式 | 数量 | 游戏 |
|---|---:|---|
| 服务端 Steam 创意工坊与实例合集 | 10 | 饥荒联机版、僵尸毁灭工程、Unturned、ARK 生存进化、潜渊症、流放者柯南、幻兽帕鲁、Squad、泰拉瑞亚/tModLoader、灵魂面甲 |
| 已验证的 Thunderstore 包链接与本地导入 | 3 | Core Keeper、Valheim、V Rising |
| CurseForge 项目 ID 与本地导入 | 1 | ARK 生存飞升 |
| 本地文件导入与来源链接 | 11 | Abiotic Factor、Astroneer、Enshrouded、HumanitZ、Minecraft、Necesse、Rust、Satisfactory、七日杀、森林之子、Windrose |
| 仅说明客户端依赖 | 1 | RimWorld Together |
| 暂无 LanGame 服务端 Mod 工作区 | 6 | Nightingale、Return to Moria、Romestead、RuneScape: Dragonwilds、SCUM、The Forest |

Thunderstore 与本地导入两组共 14 款文件型游戏支持查看实例库存、打开 Mod 目录，目前没有通用的启用、停用或移除操作。导入文件不会安装所需加载器，也不代表游戏已成功加载。Thunderstore 链接需通过运行环境和依赖验证；Minecraft 支持导入匹配的本地 JAR，实例加载器及版本尚未形成可靠绑定，因此阻止自动 Modrinth 安装。ASA 单独管理 CurseForge ID 清单。非 Steam 来源不使用「我的合集」。LanGame 尚未接入不代表游戏不存在外部 Mod 生态。

Manifest 的 `supports_collections` 字段目前不是界面能力开关。上述 10 个 Steam 工作流由 LanGame 将合集展开为经过核验的服务端包，字段为 false 不代表应用不能管理该游戏的合集。

ASA 的「我的 Mod」同时列出普通启用和被动加载的 ID。停用会从两份加载清单移出，并保留原来的加载方式供重新启用；尚未下载的 ID 也不会因停用而消失。移除会清除实例归属并隐藏保留的缓存文件，显式重新添加 ID 或再次导入对应文件可恢复。文件导入只恢复本次实际写入的项目，不会自动启用。自定义启动参数中含 `-mods` 或 `-passivemods` 时，需先移除这些参数再使用管理操作，避免界面停用后仍被原始参数加载。两种加载清单都会把多个 ID 规范为单个逗号分隔参数。

在仓库根目录运行 `python -B scripts/audit_mod_workflows.py --check` 可只读检查全部模块声明。它核对代码覆盖范围，不表示已成功启动验证全部 32 款服务器。

### 创意工坊与饥荒模组

「模组」页搜索当前游戏的 Steam 创意工坊。输入名称时默认按相关性排序，也可选择本周热门、评分最高、最新或订阅最多。搜索使用界面语言，支持直接输入 Workshop 项目 ID 或完整链接。翻页有加载反馈；请求失败时保留上次成功的结果，并可重试。

饥荒的「安装」会先通过 SteamCMD 下载模组文件，再保存此实例的下载列表和分片启用配置。在「我的 Mod」中选择模组，即可读取本地 `modinfo.lua` 选项。文件缺失、读取失败和模组没有声明选项会分别提示。普通模组管理统一在此工作区，通过「应用范围」选择同步两分片或分别编辑地面、洞穴；高级原始 Lua 覆盖仍在配置页。文本和数字选项在离开输入框或按 Enter 后保存。读取选项不会重写实例配置，运行中的实例保持只读。

启动会等待正在执行的 Mod 操作，包括前置读取和下载；操作失败后，需重试或重新打开已保存的配置才能继续启动。同一分片的自定义原始 `modoverrides.lua` 不能与结构化启用列表、选项同时使用。嵌套合集需通过清单模式展开，普通安装不会静默只安装合集中的部分条目。

禁用的饥荒模组仍保留在「我的 Mod」中，并保留已保存的选项。重新启用当前实例已有的条目只更新配置，无需在线查询或再次下载。同时编辑两分片时，只要实际选项值不同就会提示，包括一侧使用默认值的情况。

安装会将所选完整包及官方安装记录部署到当前实例地表、洞穴各自的缓存；共享下载缓存已存在时也会完成这一步，残缺缓存会重新下载。启动时遇到下载超时或原生模组加载错误会报错并给出分片日志，不会仅因世界已初始化就认定模组加载成功。

已配置条目会核对实际本地文件，包含饥荒各分片的 UGC 下载目录。启用勾选代表配置意图，缺少文件会单独提示。Steam 指南不能作为 MOD 安装；仅客户端的饥荒 MOD 会单独标识，并提示在游戏客户端配置。已有的无效条目保留在列表中，可从实例移除。

### 工坊 ID 清单

工坊订阅和启用模组清单统一由「模组」选项卡管理，包含潜渊症、流放者柯南、灵魂面甲和僵尸毁灭工程。僵尸毁灭工程的地图加载顺序也由「模组」统一管理，与地图安装、排序和移除保持同一入口，配置页不再重复显示这些控件。

顶部「我的 Mod」后方的「清单模式」支持粘贴工坊 ID、条目链接或集合，按换行、逗号或分号分隔。核对时按输入顺序展开集合并去重，检查所属游戏、条目类型与现有文件。「补齐下载」保留启用状态，「补齐并启用」合并到现有配置。执行前会重新检查本地文件，复用已下载包，并部署到游戏实际使用的实例目录；更新所选包时替换其旧内容，防止上游已删除的文件继续加载，同时保留其他包和无关配置。最多支持 8192 项和 1 MiB 文本。

共享工坊缓存复用要求存在对应游戏的安装记录、非空文件内容与记录字节数一致，且没有冲突的较新 manifest。空目录、无记录或大小不符的目录会重新下载；该检查不代表逐文件内容校验。

「Mod / 合集」切换也作用于实例库，合集模式显示「我的合集」。合集成员和「我的 Mod」共用同一份实例状态。饥荒、僵尸毁灭工程、幻兽帕鲁、ARK 生存进化、潜渊症、流放者柯南和灵魂面甲支持单项及整包启停；停用后仍保留归属，可直接重新启用，无需再次下载。半选框表示仅部分成员或内部包启用。僵尸毁灭工程操作前重新读取内部 Mod ID 和地图信息，并保留其他工坊条目仍需使用的内部名称。幻兽帕鲁移除成员后记录其已移除状态，保留的包文件不会令其重新出现在实例库。

单项移除作用于当前实例中的该 Mod，并同步到所有引用它的合集；整包移除先展示成员清单，保护其他合集共用的成员，也可只移除合集记录。下载缓存和已保存选项保留。Squad 支持单项移出实际插件加载目录并保留合集快照以便补齐，中断事务可恢复；它没有独立启用开关。Unturned 的清单用于下载，tModLoader 启用需要内部 Mod 名，因此不显示工坊 ID 启停开关。tModLoader 存在已启用内部名称而缺少工坊映射时，需先在设置中停用对应名称，再移除成员。正在运行、保存中或本地信息不足时，不会假装已完成操作。

Thunderstore 安装按稳定的来源及项目 ID 管理包和归属记录。重装、更新会替换同一个包，保留用户额外添加的文件；遇到已被修改的受管文件则停止覆盖。发现早期安装遗留的未登记副本时，会给出路径供核对，防止再次安装后同时加载多个版本。

Conan 和 Barotrauma 支持加载名称以工坊 ID 开头的手动导入包。载荷缺失或来源不明确时会阻止发布残缺的原生清单。Conan 拒绝冲突的 PAK 文件名，并保护未登记或在外部修改过的原生文件。Barotrauma 更新当前实例实际使用的 `config_player.xml`，保留其他设置及核心包；只有实例文件不存在时才使用运行时目录中的初始配置。

编辑器会显示对应游戏的原生目标。饥荒、Unturned 和 Soulmask 在服务器启动时消费清单。ARK SE 同步 `ActiveMods`、`ModInstaller.ModIDS` 与自动安装；PZ 读取内部 Mod ID 和地图信息；Palworld 读取 `Info.json` 的 PackageName；Conan 和 Barotrauma 从已部署文件生成加载清单；Squad 安装插件，不虚构独立 ID 启用设置。tModLoader 的下载操作保存 `install.txt` 所需清单，但保留 `enabled.json` 的已有名称，仍需单独设置内部 MOD 名称及 tModLoader 运行环境，参见[官方专用服务器文档](https://docs.tmodloader.net/docs/stable/md__github_workspace_src_t_mod_loader__terraria_release_extras__dedicated_server_utils__r_e_a_d_m_e.html)。

### ARK SE 与 ASA 配置

在「集群传输 → 集群地图」中为同一实例添加地图。每张启用的地图运行独立原生进程，拥有独立 LanGameCMD 标签、登记端口、原生日志和世界存档，共用一套服务端程序、配置和模组。添加地图时会自动填写空白的集群 ID；默认传输目录供实例内所有地图共用。新增、暂停或移除地图前需停止实例。暂停或移除地图会保留存档；更换地图包时应新增地图条目。启动和停止作用于所有启用地图，普通世界备份覆盖整个原生 Saved 树，包含已保留的地图世界。

两代 ARK 继续使用 LGSM 的房间、网络、管理权限、运行与高级分类。属性表、玩家与生物经验曲线、印痕点数及原生规则编辑器位于对应玩法分类。可以切换原生文本；编辑会保留未知 Mod 属性和未修改的原生值。无效数值、索引或括号语法会阻止保存。等级 CSV 只追加新索引，不替换已有曲线。类名建议是共享参考，实际可用内容仍取决于版本、地图及已安装模组。

配置工具栏支持预览导入 `Game.ini`、`GameUserSettings.ini`，最多两个文件，每个不超过 2 MiB。已识别条目填入对应控件，未知分组及重复条目保留到原生覆盖；已登记网络参数和用于模组启用的 `ActiveMods` 清单继续由原工作区管理。「从其他实例复制」只接受同一 ARK 版本，可按玩法分类选择；来源未设置的覆盖值会从目标清除。玩法预设支持导出和导入，不包含名称、密码、路径、网络、集群或模组安装清单。

保存和重新生成 ARK INI 时会保留未托管条目及注释。显式原生覆盖优先；删除先前托管的覆盖值会删除对应原生条目，同时保留无关 Mod 设置。运行期间保存的修改在下次启动时物化。文件与所有权记录使用同一冲突检测和回滚事务。

同一集群的地图需要相同的 ARK 版本、「集群 ID」和「共享集群目录」。目录留空仍使用本实例的集群存储，仅填写相同 ID 不会共享上传数据。修改目录不搬移已有上传数据，支持绝对本地目录和 UNC 共享路径。ASE 与 ASA 必须使用不同传输根目录。

「维护」中的集群面板列出成员地图、实际目录和端口，诊断 ID 或路径不一致、跨版本共用目录、目录相互包含及无法检查的成员。成员变更后需刷新列表；整组启停沿用各实例的正常流程，逐项报告结果，部分失败不会显示为全组成功。

整组快照与恢复在本地桌面使用。先停止所有成员、刷新列表，并确认所选共享根目录由本集群独占。快照包含每个成员的托管配置目录、完整原生 `ShooterGame/Saved` 目录及整个已确认的共享传输根目录；其他已登记集群不能使用同一目录或其父子目录。单实例世界备份不具备这种整组一致性。

恢复前需要单独确认快照及受影响成员。LGSM 校验成员和文件校验和，先为当前整组数据创建完整保护快照，再暂存全部替换内容。发布失败时整组回滚；进程中断或回滚未完成时，待处理事务会阻止成员启动。使用明确的中断恢复入口处理：尚未提交的事务回滚到保护状态，已完整提交的事务完成清理。整组事务期间不能并发改配置或启停。快照不复制服务器程序和已安装 Mod 包。

完整配置恢复还要求实例名称、已登记端口、绑定地址及自启动设置与快照一致。失配会在数据修改前被拒绝，不会用旧配置镜像静默替换数据库登记值；请先匹配这些设置，再重试恢复。

「维护 → 崩溃恢复」提供 LGSM 已有的恢复策略：开关、连续重启上限、等待时间及退出码规则，在运行服务存活期间生效，包括关闭界面之后；主动停止会取消恢复。定时维护、游戏及模组更新属于独立操作。

### 反馈缺失或重复设置

请提供游戏模块、配置分类、原生键名或启动参数，以及发行方文档或专用服务器资料。说明预期行为；如有重复字段，请列出所在位置。示例不得包含密码、访问令牌、玩家数据或私有地址。

[配置来源台账](game-config-source-ledger.md) 和 [配置验收记录](game-config-acceptance/) 区分已实现设置、已说明的排除项及尚未验证的原生行为。这些记录说明证据覆盖范围，不保证自动支持后续游戏更新新增的全部设置。
