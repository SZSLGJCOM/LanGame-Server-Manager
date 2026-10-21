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

**Maintenance → Saves & backups → Save now** and the server-card shortcut use the module's declared `save_world` action. All modules except Squad declare an immediate save or persistence action; the command and transport come from the manifest. LanGame uses the game's existing dedicated-server command or API when available, and adds a control extension only where a suitable external entry is unavailable. A backup separately copies data already written by the game.

Minecraft, Project Zomboid, Terraria, Unturned, Necesse and Romestead send their native command through managed stdin. Don't Starve Together checks that all enabled shards are running, then sends native `c_save()` once to Master; the game broadcasts the save to the other shards. Both ARK editions use RCON `saveworld`; HumanitZ uses native RCON `save`; 7 Days to Die uses Telnet `saveworld`; Palworld uses authenticated REST `POST /v1/api/save`. Remote interfaces require their declared enable switch, password and TCP port. ASTRONEER uses its password-protected TCP console and `DSSaveGame`, keeping the connection open through a following read-only query. These routes no longer install save-only observers, JVM agents or loaders.

Games that still require a control extension prepare it before process startup, using an independent program where needed. Core Keeper, V Rising, Sons of the Forest, Abiotic Factor, Return to Moria, Windrose, Barotrauma, Nightingale, Conan Exiles, SCUM, Valheim, RuneScape: Dragonwilds, The Forest and Enshrouded retain their respective control routes. Existing gameplay Mods, player-management extensions and operator-selected loaders are preserved. Compatible managed files can be reused; unknown or externally changed files are preserved and reported as conflicts. Save now does not install or inject a loader into a running server. Current routes and limits are recorded in each module's `control/README.md`.

[RimWorld Together](../modules/rimworld/control/README.md) uses the project's native `forcesave <username>` command through the managed console; the server skips offline clients. This requests client-side saving and upload without certifying either completion. It no longer requires a .NET save startup hook. [Barotrauma](../modules/barotrauma/control/README.md) retains its .NET hook to save a loaded campaign checkpoint between rounds; the action does not end a round or call inventory-clearing `SavePlayers`. [SCUM](../modules/scum/control/README.md) retains its narrower native persistent-state scope. [Soulmask](../modules/soulmask/control/README.md) sends native Echo `SaveWorld 1` for immediate saving and separately uses `SaveAndExit 1` for normal shutdown. Native-command success is a submitted request, not proof of disk completion. Nightingale, Conan Exiles and Enshrouded retain their documented native bridges. Squad has no declared world-save action; backups can retain configuration, lists and replays.

Valheim's **World Rules** category edits startup presets, difficulty overrides and additional rule switches. Empty choices preserve saved rules or the selected preset; opening the page never fills defaults or changes configuration. Edits use ordinary automatic configuration saving and apply on the next server start. A separate read-only table shows saved rules and the projected next-start result. Native metadata versions 26–42 are supported, including build 25909431. Read failures leave configuration editable and report the diagnostic and retry action in the activity bar. Starting a new run or stopping the server refreshes the readback; unchanged runtime polling does not repeatedly read files. A selected preset replaces saved rules before applying individual overrides. Clearing an additional rule switch only removes its startup override; it cannot turn off a rule already enabled in the saved world or preset. Save and backup policies remain in Maintenance.

Valheim startup automatically prepares an independent server program, the reviewed BepInEx loader and the management extension for saving and native account collection. **Players → Prepare account management early** can perform the same setup while stopped. Startup verifies the known native assembly profile and installs or refreshes the owned management DLL under the instance and program locks. An existing DLL must match the recorded previous payload or a recognized pending update. Recognized interrupted updates can be recovered; updating LanGame does not require enabling account management again. Setup does not write world rules or saves. Externally changed and unknown files are preserved and reported.

After starting a supported native build, each online character is bound to the full, case-sensitive Platform User ID from the same native peer. Selected-player administrator and allowlist changes retain other settings, save the roster, reload native authority and verify the resulting list and online administrator state. A failed live check reports the saved change separately from its unconfirmed effect. Without the extension, character-name collection remains read-only account evidence; names cannot fill administrator or allowlist IDs. Ban-list writes do not claim confirmed live moderation: the native game may periodically reread them, and restarting guarantees their application. Enabling BepInEx changes the instance's runtime and marks Valheim as modded; removing the management plugin does not automatically remove the loader.

Enshrouded 1.0.0.1 build 25967645 supports **Flame Level for Shroud Removal** under Progression (default 11, effective range 1–11) and **Automatic** enemy amount under Combat. The packaged Version 1.0.0.0 README lists 0–20; the earlier 1.0 build 25936466 clamped 0 to 1 and 20 to 11, and the hotfix directly accepted 1 and 11. LanGame validates the effective range and retains the existing enemy-amount defaults. Before upgrading a world, stop its server and keep a verified copy of the entire old save directory outside the runtime. Version 1.0 converts the save format; retain the pre-upgrade copy independently of the native rotating backups and test recovery on another copy.

Enshrouded has 74 Configuration fields and one ban-roster field in Player Access. Role passwords, permissions and reserved slots remain under Administration & Permissions, with four preset roles and custom groups; health, survival, world, progression and combat rules stay in Configuration. Select Custom for the five gameplay sections to take effect. Individual bans use exact native accountId hashes in enshrouded_server.json's bannedAccounts array. Stop the server before changing bans; normal configuration saves preserve the existing roster, while an explicit Player Access removal unbans only its target. Native and old managed records are merged without discarding names or dates; managed JSON retains unknown metadata stripped by native serialization, and exact pre-change JSON is retained under config/native-ban-recovery/<content-hash>/. Decimal spellings such as 00123 identify the same uint64 account as 123; Steam IDs are not converted. Mods owns source links and local DLL/folder/ZIP import into the private runtime's mods directory; install a compatible community loader separately. Production staging verified three exact synthetic payloads and removed them before server launch; this does not establish loader or third-party Mod compatibility.

The isolated 1.0.0.1 native test used build 25967645, depot manifest 3715394091293008757 and SVN 1267041. Five cycles passed upgrade, restart, post-migration manual restore, pre-migration manual restore and re-upgrade, and native-backup restore. Every cycle loaded the world, reached Run, matched all 38 requested gameplay values, retained all four role groups, saved, exited 0 and produced an exact production Manual backup. The first 330-second observation retained a 42833-byte running world and ten distinct native backup generations; synthetic nonempty bans and managed metadata survived, and explicit removal affected only its target. All six original save/configuration files, their recovery copies, three immutable source saves and the original executable remained unchanged; owned processes and the disposable fixture were released. This three-file, 3634-byte world contains no persisted entities. Complex worlds, real-client gameplay or ban enforcement, community loaders, and the full Tauri start/stop and AutoStop path remain unverified; the A2S check covered only an empty roster.

RuneScape: Dragonwilds reads gameplay values from the actual saved world and presents peer sidebar categories: World, Survival, Player rules, Death and inventory, Spells and teleportation, Building, Crafting and processing, Progression and Creatures. World contains only mode and world-event frequencies; friendly fire belongs to Player rules. Stop the server and refresh before editing. Switches and sliders automatically save changes; adjusting a rule that needs Custom switches to that mode while retaining the current rules. Mode transitions are saved in order, and continuous slider edits are coalesced. Every write creates a recoverable backup, checks world identity and revision, publishes the change and reads it back. Starting waits for pending writes; invalid or failed drafts block it. Conflicts retain drafts and require refreshing current state rather than automatic resubmission. There are 63 verified existing-world controls; internal and creation-only rules remain unavailable. The Room setting **World to Load** selects the matching world with the latest internal save time. Ambiguous selection, unsupported SPUD layouts, more than 256 save-directory entries or a world larger than 128 MiB block editing and preserve the files. Native loading and persistence evidence covers 1.0 build 25501739; browser acceptance separately verifies categorization and automatic saving with a substituted transport. Separate build 25811348 probes loaded an old Custom world and its Building rule, observed an actual native autosave, and verified production stop, byte-identical AutoStop backup and restart with the same world. Its online Owner Player ID accepts at most 32 UTF-16 code units; LanGame rejects overlong values intact. The managed native console uses game-thread `quit` for orderly teardown, because Ctrl+C exited this build abnormally. Native `quit` does not create an additional world save: only the last completed autosave is recoverable, and a post-stop backup cannot include unsaved in-memory progress. Real clients, authentication and multiplayer still require separate acceptance.

Satisfactory uses the running server's native API for game-owned settings. Start the server, then complete **Room Settings → Set up server** if it is unclaimed; LanGame generates and securely stores a management password. For an existing claimed server, authorize it once under **Administration & Permissions**. That category also provides explicit viewing of a saved management password. Local commands and remote LAN management both require the existing management authority; LAN access has no separate viewer role. Room Settings contains the native server name, join password, startup world and save selector. Ordinary room changes automatically save; removing the join password is a separate explicit action. Loading a save requires confirmation and no connected players.

Under Satisfactory's **World → New World**, choose the starting location, six resource/cost generation rules and initial progress, then explicitly create and load a separate session. Generation rules cannot be changed afterward. **World → Creative Mode** contains five current-world rules and three default-player rules. Enabling Creative Mode permanently disables achievements for that save and requires acknowledgement; granted unlocks are retained when their switches are turned off. The game determines when players receive default-rule changes. Current-world rules automatically save through a serialized native queue and are read back; they do not enter ordinary instance configuration. Pending live API writes participate in the stop barrier, including after leaving the editor. A failed write is reported without preventing shutdown; it does not become a prerequisite for restarting the API that is needed to save. Native saves requested before and after a rule change are separate from recoverable backups in Maintenance. Existing pause, network-quality, weather and gameplay-data controls display concrete configured or API-read values without turning a read into a configuration write. Unread values remain explicitly unknown; supported choices can still be selected. Ports remain in Network; save/backup and restart policies remain in Maintenance; individual player actions remain in Player Access.

Satisfactory world creation and loading may be accepted before the world finishes loading. LanGame refreshes state within a bounded wait and keeps the operation pending until the selected session is observed. If the result is uncertain, refresh and inspect the current world before retrying; an accepted request is not automatically resent. Revision conflicts and errors retain drafts for review. The production backend was verified against an isolated native CL 502094 server, build 24656085, for setup, creation, room changes, rule saving and save reloading, including retention of the preceding world. That integration used isolated credential storage; it does not establish the complete GUI-to-server chain, multiplayer effects or compatibility with later builds.

ASTRONEER's **Room Settings → Startup save** lists existing native saves by their descriptive names. Choose one instead of typing a filename; it is used on the next start with automatic loading enabled. Refresh the list after adding saves through the existing workflow. An unmatched configured value is retained, and an empty list or read failure prevents replacement. Reading or choosing a startup save does not switch the running world. Custom Game rules belong to each save and are preserved; their creation and editing require the game's Owner/Admin player workflow. LanGame does not expose unsupported oxygen, power, research or planet-generation controls. Save import, immediate saving, backups and player permissions retain their existing locations.

Romestead uses a **World** size selector for Small, Standard and Large, applied only when creating a world. **Runtime & Advanced → Performance** contains the CPU-sleep switch and its 1–16.5 ms threshold, including fractional values. Turning it off writes the native no-sleep value; turning it back on restores the editor's previous valid threshold. Nightingale's existing HTTP-status switch and port remain in their configuration categories; the advanced help points to those controls. Its starting difficulty applies only to initial map creation. These presentation changes retain their existing native settings and do not claim a new server or multiplayer acceptance run.

Return to Moria's username-based permissions file is edited under **Player Access**. The editor reads the native file, including players added by the game; archived previews read the retained permissions without changing the archive. Existing-world DLC upgrades belong to **Maintenance → Existing-world DLC upgrade**, separate from DLC selection for a new world. Both editors require a stopped instance and an explicit save; changes apply on the next start. Upgrading an existing world cannot be undone, so back up the world first and check that participating players own the DLC.

SCUM's partial, gold and full wipe switches are under **Maintenance → Startup wipe switches**. Enabling a switch requires confirmation, followed by an explicit save while the instance is stopped. The native server performs the selected removal when started; saving the switches does not itself erase data. LanGame retains saved switches until you explicitly turn them off, so check them before every start. They are separate from automatic world-save and backup policies.

Immediate world saving belongs to **Maintenance → Saves and backups**, and broadcasts belong to **Maintenance → Broadcasts and automatic rules**. Minecraft, Terraria, Project Zomboid and Palworld no longer show a separate Tools page for these maintenance actions. Immediate saving asks the game server to persist the current world; creating a backup retains a recoverable copy.

DST broadcasts use this same maintenance panel and require the Master shard to be running. The message is quoted as Lua string data and sent through the managed Master console using the game's native `c_announce`. If process input confirmation is delayed, the same history entry stays **Awaiting confirmation** until the original writer confirms success or failure; the active history refreshes automatically. **Sent** confirms transport writing, not receipt by every player. An application restart leaves unresolved entries pending. The original submission is never retried automatically; pending entries count toward the existing lifecycle cooldown, while later lifecycle operations still follow that configured cooldown. Check game logs before manually resending.

Tools report command delivery separately from game effects and retain server responses, including rejection text. If console writing is still pending, check the runtime log before retrying; a batch stops at the first unconfirmed write or transport failure. RCON and REST tools require the corresponding instance settings and TCP port, followed by a server restart when those settings change. Project Zomboid uses its configured RCON port and password without a separate enable switch.

Palworld readiness can also use its enabled, authenticated REST API when native logs omit the ready line. LGSM checks an active world with advancing uptime and verifies that the current managed process owns the API endpoint. Disabled REST keeps log-based detection; existing warnings and errors take priority.

Both ARK editions use a server extension to spawn creatures and independently read them back by their native entity IDs. Stop the instance, install the extension from the local desktop Tools tab, then start the instance. Installation targets this instance's independent program directory and stops on unknown loaders or modified managed files. On first startup or after a game update, ASA sends the server EXE SHA-256 hash to the maintainer CDNs listed in the installation prompt to obtain matching symbol files; installation requires consent to those downloads. Enter world X/Y/Z coordinates in centimeters, rather than map latitude and longitude. Wild spawning does not require an online player; taming requires a real online character's numeric Player ID. Base level excludes bonus taming levels. Short class names resolve creatures already loaded by the map; unloaded or ambiguous Mod classes require a full `/Game/…_C` path and installed, enabled content. Results show the actual level, entity ID and position. Check the server before retrying an unconfirmed spawn or read-back.

An installed ASA extension may need fresh offsets after a server update. LGSM allows up to 15 minutes from each process start for a confirmed current-executable cold-cache download, then retains the normal 5-minute listener check. Each map must still own its required ports; old logs or a completed download alone do not establish readiness.

ARK reward actions use the numeric in-game Player ID, not a Steam ID. DST player indices belong to the selected shard's `c_listallplayers()` output. Inventory delivery and revival address that shard explicitly. The existing Tools forms list only enabled shards, including Islands and Volcano for Island Adventures, and require the selected shard to be running. Entities without an inventory-item component must be spawned nearby instead of placed in inventory. Precipitation can be snow in winter.

Player identities, access lists, bans, and administrator actions have separate support requirements. Check the [player capability matrix](player-center-capability-matrix.md) for the selected game.

Immediate-save results have an explicit `save_completion` state. `requested` ends normally with **Save request sent. The game will perform the save.** It means the native request was submitted or processed, not that all world data reached disk. `confirmed` remains available for adapters that provide their documented completion result and displays **Save completed**. `unconfirmed`, missing status, timeout and transport errors report an unknown outcome or the specific error. LanGame never automatically resends an ambiguous save. `write_confirmation_pending` concerns transport writing and cannot establish persistence. A backup, real-player recovery and power-loss durability are separate concerns.

The native-command routes do not impose the retired save observers' fixed-build, clean-plugin or writer-graph certification restrictions. In particular, the 7 Days to Die Telnet action preserves EAC, and Terraria keeps the selected vanilla or tModLoader runtime. This does not bypass the normal runtime, authentication or command-availability checks. Game and Mod settings remain under operator control.

For the inspected Satisfactory build, `SaveGame` is registered as an asynchronous API handler. Its own completion delegate receives the background writer's result after full serialization, file close and archive-error checks; a busy save fails this request rather than attaching it to an older operation. The adapter requires a terminal HTTP 200/204 response; HTTP 202 remains unknown because the [published API contract](https://satisfactory.wiki.gg/wiki/Dedicated_servers/HTTPS_API) defines it as still processing. The unique save name, active session and enumerated header corroborate ownership, but header enumeration alone does not prove a complete file write. This completion contract does not establish power-loss durability.

ARK validates all enabled running maps before sending once to each. If a later map fails, the result identifies already-submitted and unattempted targets without replaying the earlier saves. Other per-game prerequisites remain explicit: ASTRONEER needs a selected current world and console credentials; RimWorld Together needs connected clients; Barotrauma needs a loaded campaign between rounds.

ARK shutdown releases the RCON connection after the fixed `DoExit` command's first response, without sending another player query. Ordinary saves still require their complete response boundary. ASA has a 45-second final shutdown allowance: an isolated restart naturally exited about 43 seconds after stop began, narrowly exceeding the former 5-second save wait, 6-second exit wait and 30-second final allowance. A fresh two-cycle run passed within the new allowance without extra observation; both native exit codes were -1 with `crash_flag=false`. A response or an elapsed wait never substitutes for observed complete process-tree exit; unknown commands are not resent.

Historical acceptance of the removed save observers does not validate the replacement native-command routes. This routing change makes no new real-game, client-progress or saved-content acceptance claim. Existing game-specific evidence must be read against the exact route, version and save scope it exercised.

The 2026-10-20 follow-up fixed and locally verified Moria's shutdown hang. The fixed-build sender queues `exit` once, observes the native engine's exit request, then cancels synchronous I/O only on the unique owned native console input thread. Full executable and loaded-code checks guard the read-only native state binding. The sender's total budget is now 12 seconds instead of 5 to cover this hand-off; the module's 12-second post-command wait and 15-second final grace remain unchanged. Two production save/stop/restart cycles completed with native exit code 0 and independent process/port cleanup, without external cancellation, suspended threads or forced exit. A subsequent completion adapter uses the pinned synchronous save result plus native error and idle state. Both requests were confirmed in a fresh two-cycle local test.

Sons of the Forest now corrects the verified native backup boundary for ordinary automatic, manual and shutdown saves. A completed ZIP is published under a native-compatible seconds-plus-ordinal name without overwriting an existing file. The completed archive's length and hash are verified under a pinned file handle before old backups are rotated; the new recovery point is retained within the three-backup limit. The native corruption-recovery namespace remains unchanged. Both production cycles passed outer and inner ZIP CRC, native slot layout, world identity and exact pre-request primary-file backup checks. The restarted run reproduced manual and shutdown saving in the same second and retained two distinct complete recovery archives. Those backup checks alone do not establish completion of the requested main-world save.

Abiotic Factor's existing no-client guard validation returned no formal save receipt and does not establish real-player persistence. RimWorld Together's former hook-based no-client validation is historical and does not certify the replacement native-console route.

### Save and apply changes

1. Edit the relevant fields and resolve any validation errors. Invalid configuration blocks automatic saving.
2. Allow automatic saving to finish. A saving error or conflict means the latest edit has not been accepted.
3. If saving fails, resolve the reported cause and use **Retry save**. For a conflict, preserve the intended changes, reopen the instance to load its current settings, and review the edit before submitting it again.
4. Check the game's requirements before applying changes to a running server. Saving a setting does not establish that the game has reloaded it; settings read only at startup require a server restart.

Save-game backups and configuration saves are separate operations. Before a change that affects an existing world or save, create a backup and check the game's documented restrictions.

### Automatic instance startup

Open an instance's **Maintenance** tab to enable **Start this server automatically when the app launches**. All 32 game modules share this control; each instance saves its own choice, disabled by default. Changes save immediately, with progress, confirmation, and retry on failure. This independent setting does not rewrite game configuration or backup policy.

The choice takes effect the next time LGSM's runtime service starts, including when changed while the instance is running. Reopening the interface while that service is still running does not repeat autostart. Enabled instances use the normal server-start workflow, with failures reported in background tasks. Windows startup of LGSM is an application-level setting.

### Daily backups and restarts

All game instances use the existing Maintenance categories: **Autosave and backup policy → Daily backup** and **Runtime policy → Daily restart**. Each policy is disabled by default and independently saves its enabled state and `HH:mm` time. The time belongs to the machine running LGSM's runtime service, including when configured through LAN management. Turning a policy on saves immediately; editing its time saves when the field loses focus. Conflicting edits retain the draft and require reloading the policy.

The runtime service executes each policy at most once per local date. Closing only the interface does not stop scheduling. Missed offline or suspended periods are not replayed; work already queued while the service remains active may wait for an existing operation. Actions run sequentially, with backup before restart when both are due for the same instance and minute. After waiting for the instance lock, execution checks that the policy and observed time window are still current.

A daily backup uses the same file-copy and retention flow as **Back up now**; it does not first request an in-game save. A daily restart acts only on a running instance, uses its normal save/stop workflow and configured post-stop backup, then starts it again. A failed stop prevents startup, and an explicit Stop cancels the restart handoff. Results remain visible as completed, failed or skipped. A service restart marks an unfinished previous attempt as interrupted with an unknown outcome; it does not replay that date's attempt. Restoring an archived instance retains the configured times and disables its daily policies.

### ARK SE and ASA configuration

ASA Mod settings live in **My Mods → select a Mod → Configuration**. Parameters backed by an explicit author definition appear as form controls; the initial catalog covers Better Variants (`1254808`) `Debug` and `DebugSpawns`, with links to its author documentation. Opening the form does not write suggested defaults. ASA does not provide DST's universal `configuration_options` contract, so downloaded assets and descriptions are not executed or treated as configuration schemas.

For other parameters, choose `Game.ini` or `GameUserSettings.ini` exactly as the author specifies, then paste or import an INI fragment containing its `[section]` and `key=value` lines. Both files can be configured for one Mod. Import and preview leave saved settings unchanged until Save. The editor preserves repeated rows; missing sections, server-managed keys and conflicts with another enabled Mod are rejected. The three supported native crafting, stack and engram rule arrays merge by class identity. File selection and successful writing alone do not establish that the game or Mod recognizes a parameter.

ARK Mods inherit shared defaults initially. Following the DST layout, the right-hand pane for the selected Mod lists all primary and additional maps under **Enabled maps**. Multiple maps can be selected: selecting two of three applies only this Mod to those two maps, while other Mods continue inheriting shared defaults. An empty selection disables this Mod on every configured map. Parameter editing independently targets shared defaults or selected maps; saving changes only this Mod and the selected INI file. Missing per-Mod and per-file overrides continue following shared defaults, including later additions. Mixed saved values are identified before applying one draft to multiple maps. Resetting enablement and parameters are separate actions. Paused maps retain their configuration. Stop every map before saving. Only enabled or passively loaded Mods contribute their fragments to native files; disabling or removing a Mod retains its saved options and removes its managed entries. Re-enabling restores them. Each file is limited to 64 KiB of UTF-8 text, with 256 Mod configurations and 1 MiB total per effective map configuration. INI import accepts UTF-8 and explicitly marked UTF-16.

Existing whole-map Mod snapshots are converted on read into sparse per-Mod overrides; the next ordinary configuration write persists the converted settings. Effective active/passive lists, order, saved options and retained native content are preserved. A map no longer freezes unchanged Mods against later shared changes.

When enabled maps differ in their effective Mod IDs, load order, passive loading, or active Mod options, the Mods workspace displays a cross-map compatibility warning. This does not block configuration or classify individual Mods. Custom creatures, items, and character data may require corresponding Mods and compatible options on the destination server; follow the Mod author's requirements. A matching list alone does not establish transfer compatibility.

If any map uses per-Mod overrides, each configured map receives owned `config/ark-maps/<process-key>/Game.ini` and `GameUserSettings.ini` files. Startup binds those files using Unreal dedicated-server `-GameINI` and `-GameUserSettingsINI` overrides; only their path values are quoted on Windows so spaces remain readable by ARK's native command-line parser. Shared server options and world-save identities are preserved. Unknown native INI entries, encoding, and line endings retain the existing merge and rollback protections. Custom INI/user-directory overrides cannot redirect this managed configuration. Removing a map clears its Mod override while retaining its saved world.

The category override contract is documented by [Epic](https://dev.epicgames.com/documentation/unreal-engine/configuration-files-in-unreal-engine) and [an ASE configuration example](https://survivetheark.com/index.php?/forums/topic/589004-cluster-complication-detailed/&page=2). The [ASA engine API source](https://arkserverapi.wiki/asa/_config_cache_ini_8h_source.html) explicitly enables command-line INI overrides for dedicated-server Shipping builds. Rendering and process arguments are regression-tested; actual third-party Mod behavior, including Mods that hard-code native paths, and cross-map transfers require native gameplay verification.

ASA reports ready after its current map processes advertise for join. The earlier successful-start banner and open ports can precede completed world initialization; they do not end the starting health state.

Stopping saves every live map before sending exit commands. If a map closes RCON during exit, LanGame confirms that map's complete owned process tree has exited, then continues with the remaining maps. Success still requires every instance process tree to exit. A retry after partial exit uses the same launch session to stop surviving maps; a lost reply does not replay the command or authorize a forced stop.

Open **Cluster Transfers → Cluster maps** to add maps to one ARK instance. Each enabled map runs as its own native process with separate LanGameCMD output, registered ports, native log and world save, using the same installation and shared server defaults, with optional per-Mod map overrides. Use the fixed-width selector at the console's upper right to switch maps; its tooltip shows the full server name. A blank Cluster ID is filled automatically when adding maps; the default transfer directory is shared within this instance. Stop the instance before adding, pausing or removing maps. Pausing or removing a map keeps its saved world; use a new map entry when changing its package. Start and Stop operate on all enabled maps, and ordinary world backups cover the complete native Saved tree, including retained map worlds.

Enter the complete **Server name** for each map, including the primary map. The primary name is the same setting shown in **Room**; additional map names are independent. Launch uses each name directly, without adding a shared prefix or map suffix. Older configurations retain their previously effective full names when read, and the next normal configuration write saves the current name format. Renaming does not change map identities, ports or saved worlds.

ASA supplies each map's registered query port as an independent `-QueryPort=<port>` argument, retaining `?QueryPort=<port>` in its ARK URL and the managed INI value. [Epic's Unreal Engine 5.5 keyword contract](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-command-line-arguments-reference?application_version=5.5) defines `QueryPort` for OnlineSubsystemSteam, whose default is 27015. In the isolated Windows ASA 94.29 comparison on 2026-10-18, URL-only processes without whole-URL quotes initialized Steam and owned their configured query UDP port, with and without AsaApi 2.03. Keeping the same API and configuration but quoting the complete URL containing a server name with spaces produced Steam initialization `FAILED` and no configured query listener. Two concurrent processes retaining those quoted URLs and adding independent keywords each owned their declared query ports, 47015 and 47025. This comparison establishes query socket ownership for these invocation forms. ASA manages this parameter for both single-map and multi-map instances; raw launch flags cannot override it. Other supported expert arguments and ASE's existing behavior remain available.

Before launch, LanGame reads and verifies the instance's Windows Firewall rules. Correct existing rules need no administrator prompt. Changes requested by an unelevated process use a one-shot UAC helper restricted to validated instance rule data; the original process independently reads the rules back after the helper exits. Each snapshot verifies all seven filter categories. When Windows explicitly denies full enumeration, the same ordinary token reads the targeted rules through the associated-rule API; unknown failures remain errors. Processing after approval has time limits; Windows' approval wait is outside those limits. A failure to configure a specific local-address scope blocks startup. A wildcard bind keeps the existing policy: report the firewall failure and continue, with a warning that external connections may be blocked.

The 2026-10-18 Windows native acceptance started an existing ASA 94.29 instance with three maps through an unelevated manager, beginning with no instance rules. All 12 rules were configured and independently verified; the helper exited, both manager processes retained ordinary tokens, and startup recorded no firewall failure. An ordinary-token check of 12 existing rules completed in 8.9 seconds without changing their identities. Each map owned its configured query port (27017, 27018, 27019), initialized Steam successfully and advertised; cached Mods 1715971 and 941450 were checked and validated on all three maps. Settings values were preserved. The host's already-disabled firewall profiles were preserved, so this does not verify packet filtering with enabled profiles. A2S INFO did not respond on LAN or loopback; client joining remains unverified.

Returning to **Runtime** restores the console's recent history and reads the latest 400 lines from its owned log. Session history is isolated by instance, map, run and log source; a new run or service recovery cannot reuse another stream's output. Stopped maps remain selectable for reading, with commands disabled for that map.

Both editions retain LGSM's shared Room, Network, Administration, and Runtime categories. Attribute tables, player/creature experience curves, engram points, and native rule editors live in their gameplay categories. Switch to native text when needed; edits retain unknown Mod properties and unedited native values. Invalid numeric values, indexes, or tuple syntax block saving. Level CSV imports append new indexes without replacing existing curves. Class-name suggestions are a shared reference; availability still depends on the edition, map, and installed Mods.

Saving or regenerating ARK INI files preserves unmanaged entries and comments. Explicit native overrides take precedence; removing a previously managed override removes its native entry while retaining unrelated Mod settings. Pending edits made while the server runs are materialized when it next starts. These writes and their ownership records use the same conflict detection and rollback transaction.

Keep the fixed `Version=5` entry under `[/Script/ShooterGame.ShooterGameUserSettings]` in `GameUserSettings.ini`. This is the native file-format version. Without it, the game can recreate the file, dropping administrator and join passwords and leaving RCON unavailable. Two consecutive two-map start/save/stop cycles on Windows ASA build 25814268 verified password retention and each map's RCON listener. The native INI remained UTF-8 without a BOM in this acceptance; UTF-16 compatibility is covered separately by synthetic materialization tests.

Without per-Mod map overrides, maps share the native INI, so the last map to exit may write its own `RCONPort` back into the file. Each map's registered port remains independent. Before the next start, LanGame materializes the managed configuration and supplies each map's correct RCON port in its startup URL.

Maps in one cluster need the same ARK edition, **Cluster ID**, and **Shared Cluster Directory**. An empty directory retains the instance's own cluster storage; matching IDs alone do not share uploaded data. Changing a path does not move existing uploads. Absolute local directories and UNC shares are supported. ASE and ASA must use separate transfer roots.

The cluster panel in **Maintenance** lists member maps, actual directories, and ports. It reports mismatched IDs or roots, shared roots used by another edition, overlapping directories, and members it cannot inspect. Refresh the group after changing membership. Group start and stop use each instance's normal lifecycle and report each member's result; a partial failure is not presented as whole-group success.

Cluster snapshots and restoration are available in the local desktop. First stop every member, refresh the member list, and confirm that the selected shared root belongs exclusively to this cluster. A snapshot includes every member's managed configuration directory, complete native `ShooterGame/Saved` tree, and the entire confirmed shared-transfer root. Another registered cluster cannot use the same root or a parent/child of it. A single-instance world backup does not provide this whole-group consistency.

Restore requires a separate confirmation of the snapshot and affected members. LGSM verifies membership and file checksums, creates a full protection snapshot of the current group, and stages all replacement data before publication. A publication error rolls back the group; if the process is interrupted or rollback cannot finish, the pending transaction blocks member startup. Use the explicit interrupted-restore recovery action: an uncommitted transaction rolls back to the protected state, while a fully committed transaction finishes cleanup. Configuration and lifecycle changes cannot run concurrently with the group transaction. Snapshots do not copy server executables or installed Mod packages.

Complete configuration restoration also requires the instance name, registered ports, bind address and autostart setting to match the snapshot. A mismatch is rejected before changing data; restore does not silently replace database registration with an older configuration mirror. Match those values before retrying.

**Maintenance → Crash recovery** exposes LGSM's existing recovery policy: enablement, consecutive restart limit, waiting time, and exit-code behavior. It applies while the runtime service is running, including after the interface closes. Explicit Stop cancels recovery. This is separate from scheduled maintenance and game or Mod updates.

### Instance isolation

Every instance owns its configuration, saves, logs and instance Mods. Minecraft vanilla supports a shared program installation, with an independent-install option when creating an instance. Other modules currently use independent installations. Independent creation reuses verified program files from available libraries, instances or archives; an unused clean library may become the first instance's exclusive program source. The downloaded library and excluded private data are retained. By default creation verifies original program files; repairing an incomplete source can require an official installation. **Use existing local program** permits an independent instance to import program modifications without claiming official provenance. Declared configuration, save and Mod exclusions still apply. **Maintenance → Directories and diagnostics** shows verified paths, shared, private or damaged status, and conflicts with other registered instances.

SteamCMD server installation, update and validation default to anonymous. A confirmed subscription or license rejection automatically opens the same LanGameCMD login console for the original instance, or for the selected library game when no instance exists. Password and Steam Guard challenges use that console; verified login continues the original operation once. Generic, network and local file errors keep their diagnostics without requesting login. There is no separate account entry in installation headers. Authenticated downloads are local-desktop operations and verify the selected identity on every native attempt. A successful server download grants only an in-memory authorization for that exact target and program: a subsequent pre-start update may reuse it once after an anonymous permission rejection. Selecting anonymous revokes future reuse; another login or storage context invalidates it. Without that authorization, unattended startup never waits for login. Steam determines account entitlement; login alone does not establish ownership or download access.

Steam library downloads register their acquisition before the first attempt; an interruption retains that marker. Ordinary installation into a new empty directory does not request an extra SteamCMD `validate`. Acquisition retries with existing files, existing installations without an inventory, and explicitly requested validation still use `validate`. Successful acquisition checks the exact installed depot inventory and every official file hash before publishing the reusable package inventory and clearing the marker. For files of at least 4 MiB, current verification evidence is captured from the same verified handle while writes and replacements remain excluded. Smaller files are hashed again, avoiding the per-file cost of capturing filesystem evidence. On local NTFS/ReFS, publication rechecks the volume, file identity, size, modification time, and change time; unchanged admitted files reuse this operation's result, while changed files or files without reliable evidence are read again. Unsupported filesystems retain full content verification. Explicit validation never skips content checks using a historical cache.

Installation activity distinguishes native Steam validation, original-file verification, package rechecking, and inventory publication. File processing reports measured bytes, file counts, and a percentage for the current stage, separate from download speed and overall installation completion. Inventory publication reports actual save progress; Ready follows successful result persistence. Same-version validation refreshes the known Steam installation record while continuing to check program hashes. If an earlier successful download still has a pending marker or lacks its package inventory, creation can recover its complete registered build locally from the exact cached depot manifests. Missing or mismatched evidence preserves the files and asks for library validation; it does not start another whole-package download for that completed acquisition. Extra operator files are retained and never added to the original-program inventory.

SteamCMD can refuse an old installed depot manifest while downloading the current version, reporting `Access Denied` followed by `No connection`. If the current attempt's log identifies the same app, depot and manifest recorded before the attempt, LGSM preserves the original and failed app manifests in a `steamapps/.langame-manifest-recovery-*` directory and retries once with validation. The recovery keeps the depot cache and only moves the app manifest; SteamCMD then verifies the existing program files. The activity log records the backup location. Failed or cancelled recovery remains incomplete and keeps its manifest backups; it does not authorize starting an unverified installation. Back up custom changes to official program files before validation, which can replace those files.

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

Each instance owns a separate `config/clusters/main` cluster. The standard layout uses `Master` and optional `Caves`; the Island Adventures layout uses the fixed `Master`, `Caves`, `Islands` and `Volcano` roles. The official dedicated-server executable generates a new world on its first launch from the configured world-generation rules, or loads the existing native save. Generating a world in the game client first is optional. Editing generation rules does not regenerate an existing world.

Every DST instance has a private `runtime` program directory. Saves and configuration belong to the instance; each enabled shard has its own `modoverrides.lua` enablement/options and Mod files under `data/ugc/<shard>`. The download list serves the shards within this instance. Across instances, Mod installation shares only the reusable machine download cache, not enabled lists or option values. Missing or invalid runtime directories block operations. Detail reconciliation, launch preview, and startup can restore a validated rollback directory left by an interrupted update while the instance is stopped; they do not rebuild a missing runtime from shared files.

Creating an instance opens its settings without generating a map. Set the complete Surface and Caves **World generation** parameters there, then select **Start**. Startup waits for pending configuration saves; invalid settings, save failures and conflicts prevent launch. LGSM identifies existing saves and verifies that the configuration and save state remain unchanged before launching. The official server then generates new worlds or loads existing saves directly. If the configuration or save state changes during startup preparation, retry the start. Unrecognized save data must be checked before startup. Custom Lua scripts determine their own effective generation parameters and can be reviewed under Advanced settings.

To import an existing world, stop the instance and open **Maintenance → Import save**. See the [save migration guide](game-save-import.md) for the other games. Configuration retains the world-generation parameters; importing a save is a separate maintenance operation. World import requires every enabled shard and a non-empty snapshot/.meta pair in the session referenced by its native `shardindex`. Island Adventures imports require valid saves for all four fixed roles; unrecognized shard directories are rejected. LGSM validates the source, creates a backup available in **Backups**, stages the complete replacement, and restores the previous cluster if publishing fails. Instance configuration and Mod settings are preserved. A missing disabled Caves shard clears that shard's old world so it cannot later be mixed with an imported surface world. Source and destination must not overlap.

Startup checks online token requirements, mode consistency and LAN player ports (`10998`–`11018`). Draft settings can be saved before an online token is entered. A server becomes ready only after each enabled world is initialized and Master confirms every enabled secondary shard connection. Native configuration errors and unknown presets fail startup. LanGameCMD shows each enabled shard's logs during generation. Five minutes without recognized generation progress, or fifteen minutes overall, ends the wait with the relevant shard logs. Per-shard Workshop caches live under `data/ugc`, outside world backups and other instances' caches.

If one shard crashes, the remaining shard stays visible and controllable, with its log stream intact. **Stop** asks every surviving shard to save and shut down, waits up to 90 seconds for native save completion and process exit, and retains the real exit code. Missing confirmation or an abnormal exit reports failure and skips the automatic backup. Surviving processes stay managed; they are not silently killed after an unconfirmed save.

When automatic restart is enabled, a shard crash first saves and stops surviving shards, then restarts the complete instance from its saves. Unconfirmed saves prevent recovery. The restart limit counts failed sessions independently of the eight entries displayed in history. Explicit Stop cancels pending recovery; unexpected zero-code exits follow the configured exit policy.

**Maintenance → Saves and backups → Save world now** and the instance-card shortcut use the same DST operation. Every enabled shard must be running. After checking every enabled shard, LanGame sends native `c_save()` once to Master through managed stdin; the native autosaver broadcasts the save to the other shards. LanGame does not inject a `SaveCurrent` observer or compare native snapshot counters. A successful submission returns `requested`; the game owns serialization and cross-shard saving. Unknown delivery is not automatically retried, and this operation creates no separate backup. See [the save route](../modules/dontstarve/SAVE.md).

**Players → History** lists retained native join, leave and chat records, with player/account/text, event-type and time filters and 50 records per page. The shared history view currently has a DST log collector. Collection continues in the runtime service and reads each retained shard log incrementally; opening History does not manufacture activity from the current player list. Dates are estimated from process start and native log elapsed time. Online duration is shown only for unambiguous matching join/leave records from the same run; missing departures do not imply that a player is still online. Missing or truncated logs are reported as incomplete history. Archive restoration preserves the collected records; the archived preview does not query live history or players.

**Tools → World map** reads terrain and online player X/Z coordinates from the selected running shard's native Lua console. Its selector lists only enabled fixed roles, including Islands and Volcano for Island Adventures. The view shows the world day, season and phase, terrain, player names/accounts and positions, with zoom and player highlighting. Positions are collected approximately every three seconds and terrain every minute while the view is visible. Hidden, failed or expired player positions are marked stale. Terrain and player collection times are shown separately. The last selected shard and each native world camera are remembered; a stopped shard can display its last collected map with historical player markers. A new run must confirm its own session before displaying terrain, and changed session or geometry cannot reuse old terrain. Offline copies are bounded to eight maps, 64 MiB total and 30 days; unavailable browser storage does not block live reads. This replays collected server snapshots, not offline save files.

The map reads minimap textures and icons from the installed game and the selected shard's enabled Mods. A server-only observer records actual native icon assignments, visibility and priority; it is installed during normal startup and does not require players to install it. Terrain kinds come from TileGroupManager in the running game, including Mod-assigned IDs; land, ocean and empty background are not inferred from terrain names. Unknown classifications and missing material mappings are reported separately. Native road control points, entities and player coordinates come from the running shard. No game artwork is bundled with LGSM. Drag or use arrow keys to pan; scroll or pinch around the pointer, double-click to zoom in (Shift to zoom out), or use `+`/`−`. The shared zoom control starts at 50% of the fitted world and extends its upper range for large worlds to show close terrain detail; `Home`/`0` restores the complete map. Search uses declared catalog names or original prefab IDs and finds objects even when overlap hides their icons. Selecting an overlapping marker opens its actual nearby objects for individual selection. Selecting a player follows the same user ID across enabled running shards, confirming their actual shard-local presence; stale or missing source positions trigger bounded background position queries. Cancellation or manual navigation stops following. Landmark artwork shrinks with the map and is capped at 24 screen pixels in detail views. Screen-space overlap is resolved by native priority; equal priorities favor less repeated artwork, then stable entity identity. Zooming reveals separated local objects within a 1,024-icon drawing budget. This is an administrator's full-world view without a player's exploration fog. Browser edges and roads approximate the native renderer rather than reproducing its C shaders pixel for pixel. Each terrain uses its observed base material's representative opaque color when available; custom tile-edge shapes remain approximate. Missing, corrupt, unsupported or ambiguous materials produce scoped notices while other terrain and player positions remain available. Invalid world identity or geometry still fails validation. Collection, decoding and response budgets remain bounded.

LGSM collects successful native MiniMap layer and atlas registrations in the server background, including Mod post-inits that do not update the standard terrain table. Users do not install a client collector or export/import material files. Texture decoding and bounded icon indexing use the installed game and the selected shard's enabled Mod versions. Registered icons follow the native first-registered atlas and first XML declaration for the exact element key; a broken selected source cannot be replaced by a later image. Unique icon completions retain their exact atlas and element source; broken or ambiguous candidates are reported instead of borrowing another image. Cosmetic code deliberately restricted to the game client can differ from the server's registered appearance; LGSM does not guess those mappings or re-execute Mod code under a fabricated client environment. Player positions use currently connected identities intersected with entities in the selected shard, so disconnected character entities do not remain online.

The native layout and launch parameters follow Klei's [dedicated-server setup](https://kleiforums.com/forums/topic/64212-dedicated-server-quick-setup-guide-windows/) and [command-line reference](https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide).

Shared world settings, including seasonal event switches, inherit from Master into Caves following Klei's world creation rules. Explicit advanced Caves overrides take precedence.

### Mod coverage across all 32 games

The server catalog contains 32 games; Mod support depends on each module's actual installation and loading contract. This is the complete current coverage list:

| Workflow | Count | Games |
|---|---:|---|
| Server Steam Workshop and instance collections | 10 | Don't Starve Together, Project Zomboid, Unturned, ARK: Survival Evolved, Barotrauma, Conan Exiles, Palworld, Squad, Terraria/tModLoader, Soulmask |
| Verified Thunderstore package links and local file import | 3 | Core Keeper, Valheim, V Rising |
| CurseForge project IDs and local file import | 1 | ARK: Survival Ascended |
| Local file import with source links | 9 | Abiotic Factor, Enshrouded, HumanitZ, Minecraft, Necesse, Rust, 7 Days to Die, Sons of the Forest, Windrose |
| Inventory and external Mod manager | 2 | Astroneer (AML re-integration), Satisfactory (SMM/ficsit-cli package and loader layout) |
| Client dependency explanation only | 1 | RimWorld Together |
| No LanGame server Mod workspace | 6 | Nightingale, Return to Moria, Romestead, RuneScape: Dragonwilds, SCUM, The Forest |

The file-based Thunderstore and local-import workflows expose instance inventory and **Open Mod folder**. Managed packages also support enable, disable and deletion with ownership checks; existing recognizable packages can be explicitly adopted. Disabling moves the package outside the loader's scan tree. Deletion retains configuration, modified and unowned files, and refuses known dependency conflicts. See [Mod management](mod-management.md#english) for scope and recovery. Local file import does not install its required loader or prove that the game loaded it. Core Keeper, Valheim and V Rising share the Thunderstore installation workflow described below, with game-specific runtime and dependency verification. Minecraft accepts matching local JAR files, while automatic Modrinth installation is blocked without an instance loader/version contract. ASA manages its CurseForge ID list separately and retains native download caches when removing an instance entry. Non-Steam sources do not use **My collections**. A missing LanGame workflow does not imply that the game has no external mod ecosystem.

Installing Valheim or V Rising server files or creating an instance does not enable BepInEx; clicking Start automatically prepares the reviewed loader and save/control extension in that instance's independent program. With the instance stopped, dropping or pasting a Thunderstore package link verifies its game community, resolves supported package dependencies at their declared versions, then deploys them into that instance's server directory. Valheim prepares the reviewed `denikson/BepInExPack_Valheim` `5.4.2351` when needed; V Rising prepares the reviewed `BepInEx/BepInExPack_V_Rising` `1.733.2`, including its bundled `dotnet` runtime. Core Keeper uses its native Mod runtime and does not install BepInEx. No separate loader-install action is needed. The operation holds the instance and program locks, uses an independent program installation, and leaves other native instances unchanged. Unknown or conflicting loader files are preserved and reported instead of being overwritten.

Thunderstore packages route `plugins` / `BepInEx/plugins` into an owned package directory, and route `config` / `BepInEx/config` and `patchers` / `BepInEx/patchers` to their exact runtime paths. Unwrapped plugins, resources and root package metadata remain supported. Windows aliases that map to the same destination are rejected. Existing configuration bytes are preserved on installation and update, and configuration remains when disabling or deleting a package. Owned patchers follow the package through disable, enable, deletion and rollback; an update also retires removed patchers. Unowned or modified patcher conflicts stop publication without overwriting them. Online ZIPs and local imports use the same routes and ownership record; loader/core/runtime files still require the reviewed loader workflow.

Core Keeper's Thunderstore community warns that official support moved to mod.io and its old packages may be outdated. The native installation profile requires a unique, valid `ModManifest.json` at the Mod root; Thunderstore's `manifest.json` is distribution metadata and does not satisfy it. Old BepInEx loader requirements and DLL-only packages are rejected without publishing files. No currently listed native Thunderstore package has been verified; use a compatible native package from mod.io or Workshop through local import.

Thunderstore payload downloads, extraction, copying and file hashing use streaming I/O. Ordinary packages are admitted against actual available disk space rather than fixed 256 MiB download or 2 GiB extraction/hash limits. Each allocation phase checks its remaining required space; writes recheck available capacity every 16 MiB and retain 512 MiB of headroom. ZIP entry count, central-directory size, compression-ratio and path-safety checks still apply. Downloads stop after 30 seconds without data and require an average throughput of 64 KiB/s with a 120-second startup allowance. Cancelling preparation cleans up temporary downloads. Loader files, plugins and ownership records are published in the same rollback transaction; a failed publication restores the previous state, or retains recovery data and reports its location if rollback cannot finish.

Enabling BepInEx changes the selected game's runtime; Valheim's loader also marks the server as `isModded`. Individual Mods can change gameplay and client requirements. Removing the last plugin does not automatically remove or disable the loader or restore a native server. New native instances do not inherit BepInEx, bundled `dotnet`, Doorstop files or LanGame's loader ownership record from an existing installation. File deployment does not establish real game-start acceptance: start the server and check its logs to confirm that the game loaded the Mods.

V Rising's first IL2CPP interop generation can take several minutes. A verified, enabled managed loader uses the owned console to capture preloader output from the current process. With a specific bind address, current-run generation evidence admits an initialization budget of at most 15 minutes from launch; initialization completion then receives the normal 180-second listener check. These are LGSM resource bounds, not an upstream duration guarantee. Silent or failed initialization does not gain time from stale `LogOutput.log` files. Cancellation retains ownership until preparation finishes, and port verification still requires the current process's actual listener. Native servers retain their existing startup behavior.

ASA's **Manifest mode** accepts numeric CurseForge project IDs separated by commas, semicolons or newlines. **Merge and enable** keeps existing active and passive settings and saves the added IDs for ASA to download and load on the next server start. Saving the list does not confirm a download or a successful load. Invalid entries block the whole update; custom `-mods` or `-passivemods` flags must be resolved before using managed Mod controls.

For supported reference providers, the desktop drop target accepts browser links, selected links and selected IDs. Local files and directories remain available for every declared manual-import workflow. Windows uses the browser's normal drag events and a bounded WebView2 object-to-path bridge for files and directories; changing the window's native drop interception must preserve both paths. Running instances reject Mod changes.

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

「维护 → 存档与备份 → 立即保存」与实例卡片快捷按钮共用模块声明的 `save_world` 动作。除战术小队外，各模块均声明了立即保存或持久化入口，命令与控制通道以模块清单为准。游戏已有可从宿主调用的原生命令或 API 时直接使用；没有合适外部入口时才提供控制扩展。创建备份是另一个操作，只复制游戏已经写入的数据。

Minecraft、僵尸毁灭工程、泰拉瑞亚、未转变者、Necesse 和 Romestead 直接通过受管 stdin 发送原生保存命令。饥荒联机版先检查全部启用分片正在运行，再向 Master 发送一次原生 `c_save()`，由游戏广播协调其他分片保存。两代方舟使用 RCON `saveworld`，HumanitZ 使用原生 RCON `save`，七日杀使用 Telnet `saveworld`，幻兽帕鲁使用认证后的 REST `POST /v1/api/save`。远程接口需要对应开关、管理密码和 TCP 端口。异星探险家使用带密码的原生 TCP 控制台 `DSSaveGame`，保持连接直到后续只读查询返回。这些路径不再为验盘安装保存观察器、JVM agent 或额外加载器。

立即保存结果使用独立的 `save_completion` 状态。`requested` 正常结束，提示“已发送保存请求，由游戏执行保存。”，表示原生请求已提交或处理，不保证全部世界数据已经写入磁盘。仍有明确完成回执的适配器可返回 `confirmed`，提示“保存完成”。`unconfirmed`、缺失状态、超时及传输失败仍报告结果未知或具体错误，不会自动重发有歧义的保存请求。`write_confirmation_pending` 只表示传输写入状态，不能证明落盘。备份、真实玩家进度恢复和断电持久性属于不同的验证范围。

此前针对已移除保存观察器的验收记录不代表新的原生命令路径已经验收。本次路由调整不新增任何真实开服、客户端进度或存档内容恢复的验收声明；历史证据须结合当时实际使用的路径、版本及保存范围理解。

原生命令路径不再附加旧保存观察器的固定版本、纯净插件或写入图认证限制。七日杀的 Telnet 保存保留 EAC 设置；泰拉瑞亚保留原版或 tModLoader 的既有选择。正常运行时、身份认证和命令可用性检查仍然保留，游戏与 Mod 配置由用户决定。

已检查的幸福工厂版本把 `SaveGame` 注册为异步 API：本次请求自己的完成回调要等后台完整序列化、文件关闭及写入错误检查后才返回；保存繁忙时拒绝本次请求，不会加入上一轮操作。适配器要求 HTTP 200/204 终态成功响应；[公开接口契约](https://satisfactory.wiki.gg/wiki/Dedicated_servers/HTTPS_API) 中仍表示正在处理的 HTTP 202 保持未确认。唯一存档名称、当前 session 和存档列表中的文件头只作归属核对，单靠文件头出现不能证明完整写入。这条完成确认也不代表断电后的持久性。

仍需控制扩展的游戏在启动前准备相应组件，按需使用独立程序目录。护核纪元、夜族崛起、森林之子、非生物因素、重返莫瑞亚、Windrose、潜渊症、夜莺传说、流放者柯南、SCUM、英灵神殿、符文世界：龙之荒野、森林和雾锁王国保留各自的控制路径。已有玩法 Mod、玩家管理扩展及用户选择的加载器配置保留。完整兼容的受管文件可复用；未知文件及外部修改保留并报告冲突。点击保存不会向运行中的服务器安装或热注入加载器。当前入口与限制见对应模块的 `control/README.md`。

[环世界联机](../modules/rimworld/control/README.md) 通过受管控制台调用该项目的原生 `forcesave <username>`，服务器自行跳过离线客户端；请求客户端保存上传，不认证客户端后续完成，也不再安装 .NET 保存启动钩子。[潜渊症](../modules/barotrauma/control/README.md) 保留 .NET 钩子，仅保存回合间已加载的战役检查点，不结束回合、不调用会清空背包的 `SavePlayers`。[SCUM](../modules/scum/control/README.md) 保留其原生持久化状态范围。[灵魂面甲](../modules/soulmask/control/README.md) 手动保存直接发送原生 Echo `SaveWorld 1`，正常停止另走 `SaveAndExit 1`；命令成功表示请求已发送，不保证全部写盘。夜莺传说、流放者柯南和雾锁王国保留各自文档记载的原生桥。方舟先核对全部启用且运行中的地图，再逐图单次发送；部分失败列明已发送及未尝试目标，不重放之前的请求。战术小队没有声明世界保存动作，备份仍可保留配置、名单和回放。

Valheim 的**世界规则**分类编辑启动预设、难度覆盖与额外规则开关。空选项保留存档或所选预设的规则；打开页面不会回填默认值或修改配置。修改沿用普通配置自动保存，在下次启动服务器时生效。独立只读表显示存档规则与下次启动的预计结果，支持元数据版本 26–42，包括 build 25909431。读取失败仍可编辑配置，详细错误与重试操作显示在动态栏。新运行和停服会刷新读回，相同运行状态的轮询不会反复读取文件。选择预设会先替换存档规则，再应用各项覆盖。取消额外规则勾选仅移除启动覆盖，不能关闭存档或所选预设中已经启用的规则。存档与备份策略继续位于维护。

Valheim 启动时自动准备独立服务端程序、已审核的 BepInEx 加载器及管理扩展，用于原生保存与平台身份采集。**玩家 → 提前准备账号管理**可在停服时提前完成同一流程。启动沿用实例锁和程序锁，核验已知原生程序集版本组合，安装或更新自有管理 DLL；已有 DLL 的哈希须匹配归属记录中旧版或已知待提交更新。已知中断更新可以恢复，升级 LanGame 后无需手动重新启用账号管理。准备过程不写入世界规则或存档，外部修改和未知文件会保留并报告。

启动受支持的原生版本后，在线角色会绑定同一原生会话提供的完整 Platform User ID，保留大小写。选中玩家后的管理员、白名单修改保留其他设置，保存名单后重载原生权限，并核对原生名单及在线管理员状态。即时核验失败会区分已保存的配置和未确认的效果。未启用扩展时，角色名不能替代管理员或白名单的账号 ID。封禁名单保存不代表即时封禁已核验：游戏可能定期自动重读，重启是保证使用新名单的时点。BepInEx 会改变该实例的运行环境并将 Valheim 标记为 modded；移除管理插件不会自动移除加载器。

雾锁王国 1.0.0.1 build 25967645 支持进度中的**驱散瘴气所需灵火等级**（默认 11，实际生效范围 1–11）及战斗中敌人数量的**自动**选项。随包 Version 1.0.0.0 说明写 0–20；此前 1.0 build 25936466 会将 0 调整为 1、20 调整为 11，本次热修包直接接受 1 和 11。LanGame 按实际范围校验，保留现有敌人数量默认值。升级世界前应停服，将完整旧存档目录复制到运行目录之外并核验；1.0 会转换存档格式，迁移前副本应独立于原生轮转备份保留，并另用副本试恢复。

雾锁王国共有 74 项配置及 1 项归玩家管理的封禁名单。角色密码、权限和预留名额归管理权限，提供四种预设角色及自定义组；生命、生存、世界、进度及战斗规则归配置，五个玩法分区需选择 Custom 预设才生效。个人封禁使用 enshrouded_server.json 的 bannedAccounts 数组及精确的原生 accountId 哈希。更改封禁前需停服；普通配置保存保留已有名单，只有玩家管理中明确移除目标才会解封。原生与旧托管记录合并时保留姓名和日期，托管 JSON 继续保留被原生序列化移除的未知元数据，更改前的完整 JSON 留在 config/native-ban-recovery/<内容哈希>/。00123 与 123 表示同一个 uint64 账户，不转换 Steam ID。模组区负责来源链接及 DLL／文件夹／ZIP 导入到独立运行目录的 mods；兼容的社区加载器仍需另行安装。生产暂存入口已验证三个合成载荷逐字节一致，并在服务器启动前移除；这不代表加载器或第三方模组兼容性已通过。

隔离原生测试使用 1.0.0.1 build 25967645、depot manifest 3715394091293008757、SVN 1267041，完成升级、重启、迁移后手动备份恢复、迁移前手动备份恢复并重新升级、原生备份恢复五轮验证。每轮均完成世界反序列化并进入 Run，38 项请求的玩法值与四组角色均一致，保存后以 0 正常退出，生产 Manual 备份逐字节一致。首轮持续观察 330 秒，保留 42833 字节的运行中世界及十代内容不同的原生备份；合成非空封禁名单与托管元数据保留，明确移除仅影响目标账户。原有六个存档／配置文件、对应恢复副本、三个不可变源存档及原程序哈希未变，受管进程与可丢弃实例已清理。该旧世界只有三个文件、共 3634 字节，未包含持久化实体；复杂世界、真实客户端玩法与封禁执行、社区加载器及完整 Tauri 启停／AutoStop 链路仍未验证，A2S 检测仅覆盖空玩家名单。

符文世界：龙之荒野读取实际存档中的玩法参数，按用途提供并列分类：世界、生存、玩家规则、死亡与物品、法术与传送、建造、制作与加工、成长和生物。世界页仅包含模式与世界事件频率，友军伤害位于玩家规则。停服并刷新后即可调节，开关与滑条自动保存；调整需要自定义模式的单项规则时自动切换，并保留当前规则。模式转换按顺序保存，连续调节滑条会合并写入。每次保存仍创建可恢复备份、核对世界身份与存档版本，并读回确认；启动会等待未完成保存，非法值或保存失败会阻止启动。冲突保留草稿，须刷新当前状态，不会自动重发。已有世界提供 63 项已验证控件，内部规则与仅创建时可改的规则不开放编辑。房间页的**加载世界名称**选择同名世界内部保存时间最新的存档。选档存在歧义、SPUD 格式不受支持、目录超过 256 项或存档超过 128 MiB 时，阻止编辑并保留文件。原生载入与持久化证据覆盖 1.0 build 25501739；分类与自动保存另以替代传输的浏览器测试验证。build 25811348 的独立探针确认旧自定义世界及建造规则可以载入，观察到真实原生自动保存，并通过正式停止、AutoStop 备份逐字节比对和同世界重启验证。在线服主 Player ID 最多接受 32 个 UTF-16 码元，LanGame 会保留并拒绝超长值。因本版本 Ctrl+C 异常退出，受管原生控制台改用游戏线程的 `quit` 完成正常清理。原生 `quit` 不会额外保存世界，只能恢复最近一次完成的自动保存；停服后备份无法包含尚未写盘的进度。真实客户端、认证及多人效果仍需另行验收。

幸福工厂通过运行中的服务器原生 API 管理游戏保存的设置。先启动服务器；尚未认领时，在**房间设置 → 设置服务器**完成设置，LanGame 会生成并安全保存管理密码。已有服务器在**管理与权限**中使用现有管理密码授权一次，该分类也提供显式查看已保存管理密码的入口。本地命令及局域网管理均沿用现有管理权限，局域网没有独立的只读访客角色。原生服务器名称、入服密码、启动世界及存档选择均位于房间设置；普通房间属性自动保存，清除入服密码使用独立明确操作。载入存档须确认，并要求没有在线玩家。

幸福工厂的**世界 → 新世界**提供出生区域、六项资源与成本生成规则及初始进度，通过明确的创建操作生成并载入独立世界；生成规则创建后不能修改。**世界 → 创造模式**提供五项当前世界规则和三项玩家默认规则。启用创造模式会永久禁用该存档的成就，须确认知悉；关闭单项开关不会收回已获得的解锁进度，玩家默认规则何时应用由游戏决定。当前世界规则通过原生队列自动保存并读回，不写入普通实例配置；离开页面后的未完成原生写入也会在停服前等待；写入失败明确报告并仍允许停服，不会把需要运行中 API 的草稿变成下次启动的前置条件。规则修改前后请求的游戏存档与维护页的可恢复备份是不同操作。原有暂停、网络质量、天气和玩法数据控件显示具体配置值或 API 读回值，读取本身不产生配置修改；未读到的值明确显示未知，仍可显式选择支持的选项。端口仍在网络，保存、备份和重启策略仍在维护，具体玩家操作仍在玩家管理。

幸福工厂创建及载入世界可能先返回已接受，再完成载入。LanGame 在有限等待内刷新状态，在确认目标世界前保持待完成状态。结果不确定时，先刷新并检查当前世界，再决定是否重试，不会自动重发已接受的请求。版本冲突及错误会保留草稿供复核。生产后端已在隔离原生服务器 CL 502094、build 24656085 上验证初始化、创建、房间修改、规则保存及存档重载，并确认切换新世界后保留前一世界。该集成验收使用隔离的凭据存储，不代表已验证界面到真实服务器的完整链路、多人效果或后续版本。

ASTRONEER 的**房间设置 → 启动存档**按名称列出已有原生存档，无需手填文件名；启用自动加载后，在下次启动时使用所选存档。通过已有流程添加存档后可刷新列表。当前配置未匹配列表时保留原值，列表为空或读取失败时阻止替换；读取或选择启动存档不会切换正在运行的世界。自定义游戏规则随存档保留，其创建和编辑需要游戏内服主或管理员的玩家流程。LanGame 不展示未经支持的氧气、电力、科研或行星生成控件。存档导入、即时保存、备份及玩家权限继续使用已有入口。

罗马拓荒录在**世界**分类通过小型、标准和大型选择世界大小，仅在创建世界时使用。**运行与高级 → 性能**提供 CPU 休眠开关及 1–16.5 毫秒阈值，保留小数精度；关闭时写入原生禁用值，重新开启时恢复编辑器中上次有效阈值。夜莺已有的 HTTP 状态开关及端口继续使用对应配置分类，高级说明指向这些控件；首图难度仅影响初次创建地图。这些呈现调整沿用已有原生设置，不代表新增服务器或多人联机验收。

重返莫瑞亚按用户名管理的权限文件位于「玩家管理」。编辑器读取原生文件，保留游戏运行时新增的玩家；归档预览读取归档内保留的名单，不修改归档内容。旧世界 DLC 升级位于「维护 → 旧世界 DLC 升级」，与新世界创建时的 DLC 选择分开。两处编辑器均要求先停服并显式保存，下次启动时生效。旧世界升级无法撤销，操作前应备份世界，并确认参与玩家拥有对应 DLC。

SCUM 的部分擦除、金币擦除和完整擦除开关位于「维护 → 启动擦除开关」。启用开关时需要确认，停服后再显式保存。原生服务器启动时执行所选擦除；保存开关本身不会删除数据。LanGame 会持续保留已保存的开关，需手动关闭不再需要的项，因此每次启动前都应检查。这些选项与自动世界存档、备份策略分开管理。

即时保存世界位于「维护 → 存档与备份」，广播位于「维护 → 广播与自动规则」。Minecraft、泰拉瑞亚、僵尸毁灭工程和幻兽帕鲁不再为这些维护动作重复显示工具页。立即保存请求游戏服务器写入当前世界，备份则保留可恢复副本，两者用途不同。

饥荒广播复用此维护面板，要求主世界正在运行。消息编码为 Lua 字符串数据，通过受管主世界控制台调用游戏原生 `c_announce`。进程输入尚未确认时，同一条历史显示「待确认」，原始写入完成后回填成功或失败，当前页面自动刷新历史。「已发送」确认传输写入，不代表每位玩家已经收到。应用重启后未确认记录仍保持待确认。原始提交不会自动重试；待确认记录计入既有启停广播冷却，后续新的启停操作仍遵循配置的冷却时间。手动重发前请核对游戏日志。

工具页区分命令送达与游戏内生效，并保留服务器原始响应，包括拒绝信息。控制台写入仍待确认时，请先检查运行日志再重试；批次在首个未确认写入或传输失败处停止。RCON、REST 操作需要配置对应的实例开关和 TCP 端口，修改后重启服务器；僵尸毁灭工程使用已配置的 RCON 端口与密码，没有额外启用开关。

幻兽帕鲁已启用并配置认证的 REST API 也可用于就绪检查，适用于原生日志缺少就绪行的版本。LGSM 核对活动世界、持续增长的运行时间以及 API 端口的当前受管进程归属。未启用 REST 时继续按日志判断，已有警告与错误优先。

方舟两版的「生成生物」使用服务端扩展直接生成并按生物 ID 读回确认。首次使用先停服，在本地桌面工具页安装扩展，再启动实例。安装仅写入本实例独立程序目录，遇到未知加载器或被修改的受管文件会停止。ASA 扩展在首次启动或游戏更新后，需要向页面列明的维护者 CDN 发送服务器 EXE 的 SHA-256 哈希以下载匹配文件；安装前会说明并取得允许。填写游戏世界 X/Y/Z 坐标（厘米），不能使用地图经纬度；野生生物无需在线玩家，驯服生物需要填写真实在线角色的数字 Player ID。基础等级不额外叠加驯服奖励。目录短类名用于当前地图已加载的生物，未加载的或有歧义的 Mod 类需要完整 `/Game/…_C` 路径及已安装启用的内容。生成结果包含真实等级、生物 ID 和坐标；回执或读回不明时，先检查服务器，避免重复生成。

ASA 已安装的扩展在游戏更新后可能需要重新下载偏移缓存。只有本次进程的日志确认正在下载与当前程序匹配的冷缓存时，LGSM 才允许自进程启动起最多 15 分钟的准备阶段，之后仍执行通常的 5 分钟监听验收。每张地图都必须拥有所需端口；旧日志或下载完成本身不代表服务器就绪。

ARK 发放物品使用游戏角色的数字 Player ID，不是 Steam ID。饥荒玩家序号以所选分片的 `c_listallplayers()` 输出为准，发背包和复活均明确针对该分片。现有工具表单仅列出实例已启用的分片，海岛冒险包含岛屿和火山，执行时要求所选分片正在运行。不能装入背包的实体应选择在玩家身边生成。降水在冬季可能表现为下雪。

玩家身份、访问名单、封禁和管理员操作有各自的支持条件，详见对应游戏的 [玩家能力矩阵](player-center-capability-matrix.md)。

原生命令仍使用当前受管进程及实例权限。HumanitZ 需要原生 RCON 配置；异星探险家需要当前世界及控制台凭据；环世界联机需要在线客户端；潜渊症需要已加载战役的回合间大厅。这些游戏前提与已经移除的写盘观察器是不同的限制。

方舟停服会在固定 `DoExit` 命令的首条响应后断开 RCON，不再追加玩家查询；普通保存仍保留完整响应边界。ASA 的最终停服等待为 45 秒：隔离重启实测在停服请求后约 43 秒自然退出，略超原来的保存后等待 5 秒、退出后等待 6 秒加最终等待 30 秒的窗口。新的两轮隔离验收均在新预算内完成，未使用额外观察；原生退出码均为 -1，`crash_flag=false`。命令回执或等待时间届满都不能替代完整进程树退出证据；结果未知不会重发命令。

2026-10-20 的后续修复已在本机验证莫瑞亚停服恢复正常。固定版本的发送器只排入一次 `exit`，观察到原生引擎请求退出后，才取消当前受控进程中唯一原生控制台输入线程的同步 I/O；完整程序哈希和已加载代码窗口保护只读状态绑定。发送器总预算从 5 秒调整为 12 秒，以覆盖这次交接；模块命令后等待 12 秒和最终宽限 15 秒保持原值。两轮正式保存、停服及重启均完成，原生退出码均为 0，独立进程和端口清理检查通过，没有外部取消、暂停线程或强制退出。后续完成适配器通过固定版本的同步保存结果、原生错误和空闲状态共同确认；新一轮两周期本机测试均取得了对应请求的完成回执。

森林之子现在在已核验的原生备份边界修正常规自动、手动和停服保存。完整 ZIP 通过兼容原生发现规则的「真实秒时间＋序号」文件名无覆盖发布；确认长度与哈希后，持续持有文件句柄再轮转旧备份，在三份保留上限中保留新恢复点。原生损坏恢复命名空间保持原样。两轮正式验收均通过备份内外层 ZIP CRC、原生 slot 目录、世界身份，以及保存请求前主档精确备份的检查。重启轮实际复现了手动和停服保存发生于同一秒，保留了两份不同名称的完整恢复包；这些备份检查本身不能确认本次主世界保存已经完成。

非生物因素既有无客户端保护验收没有返回正式保存成功回执，不代表真实玩家持久化已验证。环世界联机此前基于启动钩子的无客户端验收属于历史记录，不能认证新的原生控制台路径。

### 保存与生效

1. 修改相关字段，并处理校验错误。配置无效时，自动保存会被阻止。
2. 等待自动保存完成。出现保存错误或冲突时，表示最新改动尚未被接受。
3. 保存失败时，处理错误原因后选择「重试保存」。出现冲突时，先保留准备修改的内容，再重新打开实例以加载当前设置，核对后重新修改。
4. 修改运行中的服务器前，确认游戏的生效要求。保存设置不代表游戏已重新加载配置；只在启动时读取的设置需要重启服务器。

存档备份与配置保存是不同操作。修改可能影响现有世界或存档的设置前，应先创建备份，并查阅游戏文档中的限制。

### 实例自动启动

在实例的「维护」页启用「应用启动时自动启动该服务器」。32 款游戏共用这一入口，每个实例独立保存，默认关闭。切换后立即保存，显示保存进度和结果，失败时可重试。此设置独立保存，不会重写游戏配置或备份策略。

设置在下次启动 LGSM 运行服务时生效，实例运行中也可修改。运行服务仍在后台时，重新打开界面不会再次执行自启动。已启用的实例通过正常服务器启动流程开服，并在后台任务中记录失败。Windows 开机启动 LGSM 属于应用级设置。

### 每日备份与重启

各游戏实例沿用既有维护分类：「自动保存与备份策略 → 每日定时备份」和「运行策略 → 每日定时重启」。两项默认关闭，分别保存开关及 `HH:mm` 时间。时间以运行 LGSM 运行服务的主机为准，通过局域网管理时也使用该主机时间。切换开关立即保存，修改时间在离开输入框时保存；并发修改冲突会保留草稿，需重新加载策略。

每项策略在同一本机日期最多执行一次。仅关闭界面不停止调度；离线或系统睡眠期间错过的任务不补跑，服务持续运行时已排队的任务可等待现有操作完成。任务依次执行，同一实例同一分钟的备份先于重启。等待实例锁后，会重新检查策略是否仍有效以及时间观察窗口是否已失效。

每日备份沿用「立即备份」的文件复制和保留流程，不会先要求游戏立即保存。每日重启只处理正在运行的实例，沿用正常保存、停服及已配置的停服后备份，再重新启动；停服失败不会继续启动，主动「停止」会取消后续重启。结果保留为已完成、失败或已跳过。运行服务重启后，前一次未结束的执行显示「结果未知（执行中断）」，当天不会重发。归档恢复保留策略时间，并关闭每日任务。

### 多实例隔离

每个实例独立保存配置、存档、日志和实例 Mod。Minecraft 原版支持共享程序，创建时也可选择独立安装；其他模块当前使用独立安装。独立实例创建优先复用程序库、其他实例或归档中的已验证程序文件；未被使用的干净程序库可成为首个实例独占的程序来源。已下载的程序库及被排除的私有数据继续保留。默认核验原版程序，修复不完整的程序来源时可能需要官方安装。明确选择「使用现有本地程序」可将程序修改导入独立实例，但不认证为官方原版，模块声明的配置、存档及 Mod 排除规则仍然生效。「维护 → 目录与诊断」显示后端核实的实际路径、共享、独立或损坏状态，以及与其他实例的路径冲突。

SteamCMD 服务端安装、更新和校验默认匿名。明确遇到订阅或许可拒绝时，自动打开原实例的 LanGameCMD 登录控制台；尚无实例的程序库下载绑定当前游戏。密码和 Steam Guard 验证均在控制台完成，身份核实后自动继续原操作一次。普通失败、网络和本地文件错误保留原诊断，不触发登录；安装页标题栏不另设账号入口。账号下载仅限本机桌面，每轮原生执行重新核对所选身份。成功的服务端下载只在内存中授权该目标和实际程序目录：后续启动前更新明确遇到匿名权限拒绝时，可用这份授权重试一次；选择匿名会撤销后续复用，换登录会话或存储上下文会使授权失效。没有这份授权时，无人值守启动不会等待登录。账号许可由 Steam 判断，登录成功本身不证明拥有游戏或下载权限。

Steam 程序库首次下载前登记未完成标记，中断时保留。全新空目录的普通安装不额外要求 SteamCMD 全量 `validate`；已有部分文件的获取重试、缺少清单的已有安装及用户主动校验仍要求 `validate`。获取成功后，核对当前版本的精确 depot 文件清单及全部官方文件哈希，再生成可复用程序包清单并清除标记。对至少 4 MiB 的文件，从仍禁止写入和替换的同一核验句柄捕获本轮证据；小文件直接重新哈希，避免逐个捕获文件系统证据的开销。在本地 NTFS/ReFS 上，保存清单前重新核对卷、文件身份、大小、修改时间和变更时间，已准入且未变化的文件可复用本轮结果，变化或缺少可靠证据的文件重新读取内容。文件系统不支持可靠身份检测时保留完整哈希复核；用户主动校验不以历史缓存跳过内容检查。

安装活动区分别显示 Steam 原生校验、原版文件核验、程序文件复核和清单保存。文件处理显示本阶段已处理字节、文件数及实测百分比，不作为下载速度或整个安装的百分比；清单保存显示实际保存进度，完成结果持久化后才显示就绪。同版本校验只刷新已知的 Steam 安装状态记录，程序文件仍核对哈希。此前已成功下载但残留标记或缺少清单的完整注册版本，创建时可依据缓存中的精确 depot 清单在本地恢复；证据缺失或不匹配时保留文件并提示先在游戏库校验，不会为这个已完成下载直接重下整个包。额外用户文件保留，不纳入原版程序清单。

SteamCMD 更新当前版本时，可能拒绝重新提供已安装版本的旧 depot 清单，先报告 `Access Denied`，再显示 `No connection`。只有本次日志中的应用、depot 和清单 ID 与尝试前的安装记录一致时，LGSM 才将原始清单和失败后的清单保留在 `steamapps/.langame-manifest-recovery-*` 目录，并额外执行一次校验恢复。恢复保留 depot 缓存，仅移开应用清单，由 SteamCMD 核验现有程序文件；活动日志会记录备份位置。恢复失败或取消时继续保留清单备份和未完成状态，不放行未经核验的安装。校验可能替换被修改的官方程序文件，此类自定义改动应事先备份。

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

每个实例有独立的 `config/clusters/main` 集群。标准布局包含地表 `Master` 与可选的洞穴 `Caves`；海岛冒险布局使用固定的 `Master`、`Caves`、`Islands`、`Volcano` 四个分片角色。首次启动时，由官方专用服务器程序按世界生成配置创建世界；已有原生存档时则加载存档。无需先在游戏客户端生成世界。修改生成规则不会重新生成已有世界。

每个饥荒实例都拥有独立的 `runtime` 程序目录。存档和配置归属当前实例；各启用分片分别使用 `modoverrides.lua` 保存启用列表和选项，模组文件位于 `data/ugc/<分片>`。下载列表供此实例的分片使用。跨实例安装模组时，共享的只是可复用的机器下载缓存，不共享启用列表和选项值。运行目录缺失或无效时会阻止操作。实例停止时，详情协调、启动预览和启动入口可以恢复更新中断留下且通过校验的回滚目录；不会用共享文件自动重建丢失的运行目录。

地表生成、地表设置、洞穴生成和洞穴设置使用 `modules/dontstarve/world-options.json` 中的官方分类与顺序，不根据字段名推测归属。清单由官方 PC 专用服务器脚本提取，记录游戏版本和脚本包哈希；显示范围遵循地表、洞穴及主世界专属规则。组内优先使用官方指定顺序，其余按当前界面的本地化名称排序。未知配置保留在「其他设置」，不会隐藏或误归入生物。Mod 自行注册的世界选项不属于这份原版清单，仍需通过高级 Lua 配置。

开发验证入口 `verify:module-settings` 会逐项检查原版清单与 schema 的四页字段和可选值。官方程序更新后，在仓库根目录运行 `python -B scripts/verify_dst_world_options.py --install-root "<DST 安装目录>"`，直接比对实际包中的字段、分类、顺序和档位；漂移会返回失败。完成差异审查后可加 `--write` 刷新清单，再同步 schema、翻译和测试。工具只读解析 Lua 数据，不执行游戏或 Mod 脚本，不会自动更改实例配置。

创建实例后会进入设置页，此时尚未生成地图。在此完整设置地表与洞穴的「世界生成」参数，然后点击「启动」。启动会等待尚未完成的配置保存；无效配置、保存失败或冲突会阻止启动。LGSM 在后台识别已有存档，并在启动锁内核验配置与存档状态未发生变化，随后由官方服务器程序直接生成新世界或加载已有存档。启动准备期间发现配置或存档状态变化时，重试即可；无法识别的存档数据需先检查。自定义 Lua 脚本的实际生成参数以脚本为准，可在高级设置中查看。

导入已有存档时，先停止实例，再打开「维护 → 导入存档」。其他游戏的迁移条件见[存档导入指南](game-save-import.md)。配置页保留世界生成参数，存档导入作为独立维护操作执行。导入必须包含所有已启用的分片，并在原生 `shardindex` 指向的 session 中找到非空快照及其 `.meta` 文件。海岛冒险导入必须包含四个固定分片的有效存档，不识别的分片目录会被拒绝。LGSM 先验证源存档，创建可在「备份」中恢复的备份，再暂存完整替换内容；发布失败时恢复原集群。实例配置和模组设置保留。洞穴未启用且源存档不含洞穴时，会清除旧洞穴世界，避免以后与新地表混用。源目录与目标目录不能相互包含。

开服前检查在线令牌、模式组合和局域网玩家端口范围（`10998`–`11018`）；令牌尚未填写时仍可保存草稿配置。所有已启用世界完成初始化，且地表确认每个启用的从世界连接后，才判定开服成功。原生配置错误或不存在的预设会使启动失败。生成期间，LanGameCMD 显示各启用分片的日志；连续五分钟没有可识别的生成进展，或总计超过十五分钟时结束等待并提示对应日志。各分片的创意工坊缓存放在 `data/ugc`，与世界备份及其他实例隔离。

某个分片崩溃后，存活分片仍可查看日志和控制。「停止」会请求每个存活分片保存并关闭，最多等待 90 秒确认原生存档完成与进程退出，并保留真实退出码。未确认保存或退出异常时会报错并跳过自动备份；存活进程继续受管，不会在保存未确认时被静默强杀。

启用自动重启后，分片崩溃会先保存并停止存活分片，再整组读档重启；未确认保存时不会继续恢复。重启上限按失败会话计数，不受界面仅展示八条历史的限制。主动停止会取消待执行的恢复；非主动、退出码为零的退出是否重启，遵循配置的退出策略。

「维护 → 存档与备份 → 立即保存」与实例卡片快捷键使用同一条饥荒保存流程，要求全部启用分片正在运行。LanGame 检查全部启用分片后，通过受管 stdin 向 Master 发送一次原生 `c_save()`，由原生 autosaver 广播协调其他分片保存；不再注入 `SaveCurrent` 观察器或比较原生快照编号。提交成功返回 `requested`，序列化及跨分片保存由游戏处理；送达未知时不自动重发，也不会另建备份。详见[保存入口](../modules/dontstarve/SAVE.md)。

「玩家 → 历史记录」展示保留的原生进入、离开和聊天记录，可按玩家／账号／文本、记录类型和时间筛选，每页 50 条。共用历史视图当前接入饥荒日志采集器；运行服务在后台增量读取各分片保留日志，打开历史不会根据当前在线名单补造活动。日期由进程启动时间与原生日志经过时间推算；在线时长仅显示同次运行中可明确配对的进出记录，缺少离开记录不代表仍在线。日志缺失或清理会提示历史可能不完整。实例归档恢复会保留已采集历史；归档预览不查询活动历史或在线玩家。

「工具 → 世界地图」通过所选运行中分片的原生 Lua 控制台读取地形和在线玩家 X/Z 坐标。选择器仅列出已启用的固定分片，海岛冒险包括岛屿和火山。界面显示天数、季节、昼夜阶段、地形、玩家名称／账号与位置，支持缩放和高亮玩家。页面可见时，玩家位置约每 3 秒采集，地形每分钟刷新；玩家位置在页面隐藏、读取失败或超过有效期时标为过期，当前状态在右侧显示，完整采集时间保留在状态提示中。返回页面会恢复上次选择的分片及各原生世界的缩放、中心；停服后可查看上次地图，玩家标记明确属于历史位置。新运行须先确认自身存档会话；会话、尺寸或地块比例变化时不能复用旧地形。离线副本最多保留 8 张、合计 64 MiB、30 天，存储不可用不阻止实时读取。这是已采集服务器快照的回放，不解析离线存档，也不新增世界生成设置。

工具页左侧复用配置与维护页的导航组件。地图占据主工作区可用高度，按正方形与实际可用宽度适应，不使用固定高度上限；视口背景透明，分片、缩放、搜索与玩家列表位于右侧并独立滚动。操作说明默认折叠，不重复展示图例、常驻刷新按钮或正常素材清单，自动采集和异常提示保留。

本机管理端的世界工具在「复活」之后提供「重抽」（重新生成世界）。确认「重新生成世界将丢失当前世界」后，先等待现有配置保存队列，若在运行则按原生流程保存并停服，随后删除各分片的 `save` 世界存档（保留 cluster 与分片配置、Mod 及设置），再按普通启动流程生成新世界，全部启用分片就绪即完成。底部动态栏显示「正在停服 → 正在生成新世界 → 完成／失败」。不创建备份，也不提供恢复；需要回档请先在「维护 → 存档与备份」手动备份。旧版本留下的 `dst-world-regeneration.json` 与 `world-regeneration/` 目录不再被读取或自动删除。

地图从已安装的游戏及所选分片启用的 Mod 中读取小地图纹理和图标。仅服务端运行的采集模块记录原生图标赋值、显示状态与优先级，在正常开服时安装，无需玩家安装；地形分类读取运行中游戏的 TileGroupManager，包含 Mod 动态分配的编号，不根据地块名称猜测陆地、海洋或空白背景；未知分类和缺少素材映射分别提示。道路控制点、实体及玩家坐标均来自运行中的分片。LGSM 不内置游戏美术素材。支持拖拽或方向键平移、以指针为中心的滚轮缩放、双指缩放、双击放大（Shift 双击缩小）及 `+`／`−`。统一倍率控件从全图适应比例的 50% 起步，大世界会自动增加近景上限，`Home`／`0` 恢复全图。按目录中的明确名称或原始 prefab 搜索并定位实际对象，也可筛选地标类型；被避让隐藏的对象仍可搜索，点击重叠标记可逐个选择附近实体。玩家跟随按同一账号跨已启用且运行中的分片确认真实本地位置；来源位置缺失或过期时有界查询其他分片，取消或手动操作退出跟随。地标图标随地图缩小，细节视角最多 24 屏幕像素；屏幕重叠按原生优先级避让，同优先级优先保留出现次数较少的素材，再按稳定实体标识决定。放大后显示分离的局部对象，绘制仍限制在 1,024 个图标内。视角为管理员全图，不模拟玩家的探索迷雾；道路与地图绘制采用近似重建，不逐像素复现游戏的 C 着色器。地块边缘从实际底图和图集的透明形状中识别 47 种九邻域过渡形状及全实心形状，按原生成功应用的顺序叠加，不按 Mod 名称、地皮编号或图集元素名称硬编码。缺失、损坏、不支持或来源冲突的素材会局部提示，其余地形及玩家位置仍可查看；世界身份或几何数据非法时仍拒绝响应。采集、解码与响应均有资源上限。

LGSM 在服务端后台记录原生 MiniMap 成功应用的图层与图集，包括未修改标准地形表的 Mod PostInit 注册。用户无需安装客户端采集模块，也无需导出或导入素材文件。纹理解码和有界图标索引使用已安装游戏及所选分片启用的 Mod 版本。已注册图标按原生规则选择该精确元素键最先注册的图集和 XML 首条声明，选中来源损坏时不借用后面的图片。图集始终完整检查 XML 结构、属性和元素键，但只校验本次所需元素的坐标；无关占位元素的非法坐标不会屏蔽其他正常图标，所需元素自身的非法坐标仍拒绝使用。唯一匹配补全会保留确切图集与元素来源，损坏或歧义候选会报告问题，不借用其他图片。补全索引完整扫描授权目录后才选择唯一来源；扫描按累计 XML 字节、目录项、深度和共用读取预算限制工作量，不以大量小 XML 文件的数量判断素材是否可用。超过预算时保留位置标记并提示，不把不完整索引当成唯一匹配。刻意仅在游戏客户端执行的外观代码可能与服务端注册效果不同，LGSM 不猜测映射，也不伪装客户端环境重新执行 Mod 代码。玩家位置取当前连接身份与所选分片实体的交集，断线后残留的角色实体不会继续显示为在线。

精确注册和唯一匹配都不存在时，可使用实际实体明确声明的物品图标补全地图图标。所有使用该图标名的地标必须声明相同来源，且该名称不能用于在线玩家图标；地标采集截断、声明不完整或冲突时保留位置标记。后续位置刷新中的玩家也不会借用这类物品素材作为头像，缺少头像时仍显示名称与位置标记。该补全不根据 Mod 或 prefab 名称猜测素材，也不跳过已经选中但损坏的来源；界面的「图标补全」保留确切图集、元素与来源类型，用户无需额外操作。

边缘图集以实际注册的 XML 与显式 TEX 文件共同确定身份，XML 中过时的纹理文件名不会替换真实图层。噪声只调制底图 RGB，忽略噪声 alpha，并在后台解码时保留零 alpha 噪声中的原始 RGB。未观察到采样设置时保留空值，使用默认九邻域重建模型；其他采样方式或不能可靠识别的图集保留已有地形填充并局部提示。绘制按每格至多九个关联图层建立稀疏任务，避免大量散布的 Mod 地皮导致逐图层扫描全图。通用机制不等于已验证所有工坊版本及任意联用组合，实服验收以实际启用版本和分片为准。

原生目录和启动参数依据 Klei 的[专用服务器配置指南](https://kleiforums.com/forums/topic/64212-dedicated-server-quick-setup-guide-windows/)与[命令行参数文档](https://support.klei.com/hc/en-us/articles/360029556192-Dedicated-Server-Command-Line-Options-Guide)。

活动开关等共享世界规则按 Klei 的建档规则从地表继承到洞穴；洞穴高级配置中的显式覆盖仍优先。

### 全部 32 款游戏的 Mod 覆盖范围

服务器目录共有 32 款游戏，Mod 能力以各模块实际接入的安装与加载方式为准。当前完整清单如下：

| 接入方式 | 数量 | 游戏 |
|---|---:|---|
| 服务端 Steam 创意工坊与实例合集 | 10 | 饥荒联机版、僵尸毁灭工程、Unturned、ARK 生存进化、潜渊症、流放者柯南、幻兽帕鲁、Squad、泰拉瑞亚/tModLoader、灵魂面甲 |
| 已验证的 Thunderstore 包链接与本地导入 | 3 | Core Keeper、Valheim、V Rising |
| CurseForge 项目 ID 与本地导入 | 1 | ARK 生存飞升 |
| 本地文件导入与来源链接 | 9 | Abiotic Factor、Enshrouded、HumanitZ、Minecraft、Necesse、Rust、七日杀、森林之子、Windrose |
| 库存与外部 Mod 管理器 | 2 | Astroneer（AML 重新整合）、Satisfactory（SMM/ficsit-cli 包与加载器布局） |
| 仅说明客户端依赖 | 1 | RimWorld Together |
| 暂无 LanGame 服务端 Mod 工作区 | 6 | Nightingale、Return to Moria、Romestead、RuneScape: Dragonwilds、SCUM、The Forest |

Thunderstore 与本地导入工作流支持查看实例库存、打开 Mod 目录。受管包还支持启用、停用和按文件归属删除；此前已存在的可识别包可以显式纳入管理。停用会把包移出加载器扫描范围，删除会保留配置、已修改和未归属文件，并阻止已知依赖冲突。范围与恢复方式见 [Mod 管理](mod-management.md#简体中文)。本地文件导入不会安装所需加载器，也不代表游戏已成功加载。Core Keeper、Valheim 和 V Rising 共用下述 Thunderstore 安装流程，并分别验证游戏运行环境和依赖。Minecraft 支持导入匹配的本地 JAR，实例加载器及版本尚未形成可靠绑定，因此阻止自动 Modrinth 安装。ASA 单独管理 CurseForge ID 清单，从实例移除时保留原生下载缓存。非 Steam 来源不使用「我的合集」。LanGame 尚未接入不代表游戏不存在外部 Mod 生态。

安装 Valheim 或 V Rising 服务端文件、创建实例时不会启用 BepInEx；点击启动会在本实例独立程序目录中自动准备已审核的加载器及保存／控制扩展。实例停止后，拖入或粘贴 Thunderstore 包链接，会核对所属游戏社区，按声明版本解析受支持的包依赖，再部署到当前实例的服务端目录。Valheim 按需准备已审核的 `denikson/BepInExPack_Valheim` `5.4.2351`；V Rising 按需准备已审核的 `BepInEx/BepInExPack_V_Rising` `1.733.2`，包含其随包提供的 `dotnet` 运行时；Core Keeper 使用原生 Mod 加载环境，不安装 BepInEx。无需另点加载器安装操作。整个操作沿用实例锁与程序锁，使用独立程序安装，不改变其他原生实例。已有未知或冲突的加载器文件会保留并报错，不会直接覆盖。

Thunderstore 包的 `plugins`／`BepInEx/plugins` 安装到独立的受管包目录，`config`／`BepInEx/config` 和 `patchers`／`BepInEx/patchers` 则按准确路径安装到运行目录。无包装插件、资源和包根元数据仍受支持；映射到同一目标的 Windows 路径别名会被拒绝。安装和更新保留已有配置的全部字节，禁用和删除也保留配置。受管 patcher 随包禁用、启用、删除和回滚，更新会撤下新版不再提供的旧 patcher；遇到未托管或已修改的 patcher 冲突会停止发布并保留原文件。在线 ZIP 和本地导入共用路由及归属记录；加载器、core 和运行时文件仍通过已审核加载器流程处理。

Core Keeper 的 Thunderstore 社区已提示官方支持转向 mod.io，旧包可能过时。原生安装配置要求 Mod 根目录有唯一、可解析的 `ModManifest.json`；Thunderstore 的 `manifest.json` 是分发元数据，不能代替原生清单。依赖旧 BepInEx 或仅有 DLL 的包会被拒绝，不发布文件。目前未验证该社区存在可用的原生包；请从 mod.io 或 Workshop 获取兼容原生包后本地导入。

Thunderstore 包的下载、解压、复制和文件指纹校验均采用流式 I/O。普通包按实际可用磁盘空间准入，不再使用固定的 256 MiB 下载或 2 GiB 解压、指纹字节上限。各分配阶段预检后续所需空间，写入时每 16 MiB 复查实际可用容量，并保留 512 MiB 余量；ZIP 条目数、中央目录大小、压缩比和路径安全检查仍然生效。下载连续 30 秒没有数据时停止，并在允许 120 秒启动余量的基础上要求平均吞吐达到 64 KiB/s。取消准备会清理临时下载；加载器、插件和归属记录在同一个回滚事务中发布，发布失败会恢复原状态，若回滚无法完成则保留恢复数据并报告位置。

启用 BepInEx 会改变当前游戏的运行环境，Valheim 的加载器还会将服务器标记为 `isModded`；具体 Mod 可能改变玩法与客户端要求。移除最后一个插件不会自动移除、停用加载器或恢复原生服。新建原生实例不会从已有安装继承 BepInEx、随包的 `dotnet`、Doorstop 文件或 LanGame 的加载器归属记录。文件部署不代表真实游戏启动已通过验收，仍需启动服务器并检查日志，确认游戏实际加载了 Mod。

V Rising 首次生成 IL2CPP interop 可能需要数分钟。已验证并启用的受管加载器通过受管控制台采集本次进程的预加载输出。绑定具体网卡时，识别到本次生成证据后，允许从启动起最多 15 分钟的初始化时间；初始化完成后，再按正常 180 秒预算核验监听端口。这是 LGSM 的资源边界，并非上游的耗时保证。没有输出或生成失败时，不会凭旧 `LogOutput.log` 延长时间。取消时会等待准备工作结束再释放操作归属，开服成功仍须以当前进程的真实监听为依据。原生服沿用现有启动行为。

ASA 的「清单模式」支持粘贴数字 CurseForge 项目 ID，以逗号、分号或换行分隔。「合并并启用」保留已有启用与被动加载配置，保存新增 ID，由 ASA 在下次开服时下载并加载；保存清单不代表下载或加载成功。存在无效条目时整批不写入；自定义 `-mods`、`-passivemods` 参数需先处理冲突，才能使用受管 Mod 控件。

支持链接解析的来源可以拖入网页链接、选中的链接或 ID；声明了手动导入的游戏仍可拖入本地文件和文件夹。Windows 使用浏览器拖放事件，并通过有界 WebView2 对象路径桥读取文件和文件夹位置；调整窗口的原生拖放接管时须同时保留这两种入口。实例运行时拒绝 Mod 修改。

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

ASA 的 Mod 设置位于「我的 Mod → 选中 Mod → 配置」。有作者明确参数定义的选项显示为表单；首批收录 Better Variants（`1254808`）的 `Debug`、`DebugSpawns`，面板附作者资料链接。打开面板不会自动写入建议默认值。ASA 没有饥荒统一的 `configuration_options` 契约，不会执行下载内容或把网页描述直接当作配置定义。

其他参数按作者说明选择 `Game.ini` 或 `GameUserSettings.ini`，粘贴或导入包含 `[节名]` 和 `参数=值` 的 INI 片段。同一 Mod 可以分别配置两个文件；导入和预览不会改变已保存设置，点击保存后才写入。重复行保持顺序；缺少节名、覆盖服务器托管参数或与其他启用 Mod 冲突时会拒绝保存。已支持的原生制作配方、堆叠和印痕数组按类名合并。选中文件并成功写入，本身不代表游戏或 Mod 识别该参数。

Mod 默认继承共用配置。参照饥荒布局，在「我的 Mod」选中一个 Mod 后，右侧「生效地图」支持同时勾选多张地图：三张中选两张只改变当前 Mod，其他 Mod 继续继承；全部取消表示当前 Mod 在所有地图停用。参数编辑单独选择共用默认或多张地图，保存只修改当前 Mod、所选地图和当前 INI 文件，不改变启用状态。已有参数不同时先提示差异；恢复共用启用设置与恢复共用参数分别操作。未覆盖的 Mod 和文件继续跟随以后共用配置的变化。暂停地图保留配置，移除地图保留世界存档。旧整图配置在读取时转换为逐 Mod 覆盖，保留当前启用、被动加载、顺序、参数及原生未知内容，下次正常写入时保存新格式。保存前需停止全部地图；仅启用或被动加载的 Mod 将配置写入原生文件。停用或移除保留已保存选项，同时撤下其托管条目；重新启用时恢复。每个文件最多 64 KiB UTF-8 文本，每张地图实际配置最多 256 份、合计 1 MiB。导入支持 UTF-8 和带 BOM 的 UTF-16。

启用地图的实际 Mod、加载顺序、主动／被动状态或参数不同时显示跨服兼容提示，不阻止保存。自定义生物、物品或角色数据可能要求目标服安装对应 Mod 并采用兼容参数；相同列表本身不能证明跨服兼容。存在逐 Mod 覆盖时，各地图使用受管的 config/ark-maps/<process-key>/Game.ini 和 GameUserSettings.ini，保留原有编码、行尾、未知内容和事务回滚；启动通过专属 INI 参数选择对应文件。第三方 Mod 硬编码路径及实际跨服传输仍需游戏内验证。

ASA 的当前地图进程开始对外广播后才显示就绪。更早的“启动成功”日志和端口监听可能先于世界初始化完成，健康状态此时仍保持启动中。

停止时先保存所有运行中的地图，再逐张发送退出命令。地图退出期间若关闭 RCON，LGSM 会确认这张地图拥有的完整进程树已经退出，再继续停止其余地图；整个实例的所有进程树退出后才报告成功。部分地图退出后重试停止，仍按同一次启动的会话识别剩余地图。丢失回包不会触发命令重放或强制结束进程。

在「集群传输 → 集群地图」中为同一实例添加地图。每张启用的地图运行独立原生进程，拥有独立 LanGameCMD 输出、登记端口、原生日志和世界存档，共用一套服务端程序和服务器默认配置；Mod 默认共用，也可针对当前 Mod 多选生效地图。通过控制台右上角的固定宽度下拉选择地图，气泡显示完整服务器名称。添加地图时会自动填写空白的集群 ID；默认传输目录供实例内所有地图共用。新增、暂停或移除地图前需停止实例。暂停或移除地图会保留存档；更换地图包时应新增地图条目。启动和停止作用于所有启用地图，普通世界备份覆盖整个原生 Saved 树，包含已保留的地图世界。

ASA 的默认地图选项仅列随服务器提供的官方地图。Mod 地图从当前实例已安装的服务端包读取作者声明的 `Preview/entry_map.txt`，同时核验插件的 CurseForge 项目 ID 和服务端载荷；不会从项目标题或文件夹名称猜地图名。选择已安装的 Mod 地图时，在同一次配置修改中仅为所选地图启用对应 Mod，其他 Mod 的继承保持不变。Club ARK 虽由官方发布，也需要安装其 Mod，因此不作为无需下载的默认地图。旧配置的地图值会保留；缺少内容时显示提示，不会自动换图或删除存档。作者未声明入口的地图仍可手动填写原生地图名，但填写名称本身不会下载内容。

启用 Mod 的 ASA 多地图实例共用原生下载缓存。启动下一张地图前，LanGame 等待上一张地图完成 Mod 初始化并拥有自己的游戏端口，避免多个进程同时解压或清理同一下载文件。等待支持取消、进程退出和有界超时；整个实例仍需通过完整端口与原生广播检查才显示就绪。未启用 Mod 的多地图实例保留原有启动方式。

返回「运行」页时恢复控制台近期历史，并从对应日志重新读取最新 400 行。会话内历史按实例、地图、运行批次和日志来源隔离；新一轮开服或服务恢复不会复用其他输出流。已停服地图仍可通过下拉查看日志，该地图的命令输入保持禁用。

每张地图（包括主地图）填写完整的「服务器名称」。主地图与「房间配置」使用同一个名称，附加地图各自独立；启动时直接使用所填名称，不添加统一前缀或地图后缀。读取旧配置时保留原来实际使用的完整名称，下次正常写入配置时保存为新的名称格式。改名不会改变地图标识、端口或世界存档。

ASA 为每张地图独立传入登记的 `-QueryPort=<端口>`，同时保留 ARK URL 中的 `?QueryPort=<端口>` 和受管 INI 值。[Epic 的 Unreal Engine 5.5 参数契约](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-command-line-arguments-reference?application_version=5.5) 将 `QueryPort` 定义为 OnlineSubsystemSteam 的独立关键词参数，默认值为 27015。2026-10-18 的隔离 Windows ASA 94.29 对照中，不给整个 URL 加引号时，仅 URL 查询端口参数即可初始化 Steam 并持有登记的查询 UDP 端口，安装和未安装 AsaApi 2.03 均成立。保持同一 API 和配置，将含空格服务器名称的整个 URL 加引号后，Steam 初始化显示 `FAILED`，登记的查询端口没有监听。保留此带引号 URL 并增加独立参数的两个并发进程，分别持有登记的 47015、47025 查询端口。本次对照确认这些调用形式下的查询端口归属。单图和多图 ASA 均由管理器维护此参数，原始启动参数不能覆盖；其他受支持的高级参数和 ASE 原有行为保留。

启动前，LanGame 先读取并核验实例的 Windows 防火墙规则，已正确配置时无需管理员确认。普通权限进程需要变更规则时，通过一次性的 UAC 提权辅助进程处理受校验的实例规则数据；辅助进程退出后，原进程再次独立读取核验。每个快照完整核验七类过滤属性；Windows 明确拒绝全库读取时，普通权限进程改用目标规则的关联读取接口，其他未知失败仍报错。批准后的处理有时间限制，Windows 等待用户审批的时间不在其中。指定本地绑定地址时，无法配置对应防火墙范围会阻止启动；通配绑定沿用原政策，报告失败并继续，同时提示外部连接可能受阻。

2026-10-18 的 Windows 原生验收通过普通权限管理器启动已有 ASA 94.29 三地图实例，从缺少实例规则的状态开始，12 条规则完成配置及独立核验，辅助进程退出，两个管理器进程保持普通权限，本次启动没有防火墙失败。普通权限核验 12 条已有规则耗时 8.9 秒，规则身份未变。每张地图分别持有登记的 27017、27018、27019 查询端口，Steam 初始化和原生广播成功；三张地图均检查并验证已有缓存 Mod 1715971、941450，用户设置值保持。主机原来关闭的防火墙配置保持，因此未验证启用防火墙后的实际报文过滤。局域网和回环 A2S INFO 均未收到响应，客户端入服尚未验证。

两代 ARK 继续使用 LGSM 的房间、网络、管理权限、运行与高级分类。属性表、玩家与生物经验曲线、印痕点数及原生规则编辑器位于对应玩法分类。可以切换原生文本；编辑会保留未知 Mod 属性和未修改的原生值。无效数值、索引或括号语法会阻止保存。等级 CSV 只追加新索引，不替换已有曲线。类名建议是共享参考，实际可用内容仍取决于版本、地图及已安装模组。

保存和重新生成 ARK INI 时会保留未托管条目及注释。显式原生覆盖优先；删除先前托管的覆盖值会删除对应原生条目，同时保留无关 Mod 设置。运行期间保存的修改在下次启动时物化。文件与所有权记录使用同一冲突检测和回滚事务。

请保留 `GameUserSettings.ini` 中 `[/Script/ShooterGame.ShooterGameUserSettings]` 节的固定 `Version=5`。这是原生文件格式版本；缺失时，游戏可能重新生成文件，丢失管理密码和加入密码，导致 RCON 不可用。Windows ASA build 25814268 同一双地图实例连续两轮启动、保存和停服，已验证密码保持及每张地图的 RCON 监听。本次原生 INI 保持 UTF-8 无 BOM；UTF-16 兼容性另由合成物化测试覆盖。

未设置逐 Mod 地图覆盖时，地图共用原生 INI，最后退出的地图可能把自己的 `RCONPort` 写回文件。每张地图登记的端口仍独立保存；下次启动前，LanGame 重新物化受管配置，并在各地图启动 URL 中传入对应的 RCON 端口。

同一集群的地图需要相同的 ARK 版本、「集群 ID」和「共享集群目录」。目录留空仍使用本实例的集群存储，仅填写相同 ID 不会共享上传数据。修改目录不搬移已有上传数据，支持绝对本地目录和 UNC 共享路径。ASE 与 ASA 必须使用不同传输根目录。

「维护」中的集群面板列出成员地图、实际目录和端口，诊断 ID 或路径不一致、跨版本共用目录、目录相互包含及无法检查的成员。成员变更后需刷新列表；整组启停沿用各实例的正常流程，逐项报告结果，部分失败不会显示为全组成功。

整组快照与恢复在本地桌面使用。先停止所有成员、刷新列表，并确认所选共享根目录由本集群独占。快照包含每个成员的托管配置目录、完整原生 `ShooterGame/Saved` 目录及整个已确认的共享传输根目录；其他已登记集群不能使用同一目录或其父子目录。单实例世界备份不具备这种整组一致性。

恢复前需要单独确认快照及受影响成员。LGSM 校验成员和文件校验和，先为当前整组数据创建完整保护快照，再暂存全部替换内容。发布失败时整组回滚；进程中断或回滚未完成时，待处理事务会阻止成员启动。使用明确的中断恢复入口处理：尚未提交的事务回滚到保护状态，已完整提交的事务完成清理。整组事务期间不能并发改配置或启停。快照不复制服务器程序和已安装 Mod 包。

完整配置恢复还要求实例名称、已登记端口、绑定地址及自启动设置与快照一致。失配会在数据修改前被拒绝，不会用旧配置镜像静默替换数据库登记值；请先匹配这些设置，再重试恢复。

「维护 → 崩溃恢复」提供 LGSM 已有的恢复策略：开关、连续重启上限、等待时间及退出码规则，在运行服务存活期间生效，包括关闭界面之后；主动停止会取消恢复。定时维护、游戏及模组更新属于独立操作。

### 反馈缺失或重复设置

请提供游戏模块、配置分类、原生键名或启动参数，以及发行方文档或专用服务器资料。说明预期行为；如有重复字段，请列出所在位置。示例不得包含密码、访问令牌、玩家数据或私有地址。

[配置来源台账](game-config-source-ledger.md) 和 [配置验收记录](game-config-acceptance/) 区分已实现设置、已说明的排除项及尚未验证的原生行为。这些记录说明证据覆盖范围，不保证自动支持后续游戏更新新增的全部设置。
