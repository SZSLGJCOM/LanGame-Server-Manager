# Development / 开发指南

[English](#english) | [简体中文](#简体中文)

## English

Set up the toolchain and desktop with [Run from source](../README.md#run-from-source). For issue reports and pull requests, see [Contributing](../CONTRIBUTING.md#english).

### Repository layout

| Path | Responsibility |
| --- | --- |
| `apps/desktop/` | React interface, translations, frontend checks, and Tauri command adapters |
| `crates/` | Domain types, storage, runtime, module integration, SteamCMD, and Windows support |
| `modules/` | Game manifests, settings schemas, native templates, and acceptance fixtures |
| `migrations/` | Workspace database schema |
| `docs/` | Operator guides, integration contracts, protocols, and generated evidence |
| `scripts/` | Portable verification, generation, and source-distribution tooling |
| `.github/` | Issue forms, pull request template, and CI workflows |

Root files have specific entry-point or tooling responsibilities:

| Files | Purpose |
| --- | --- |
| `README.md`, `README.zh-CN.md` | Product introduction and portable setup commands |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | Workspace dependencies, reproducible resolution, and Rust toolchain |
| `.editorconfig`, `.gitattributes`, `.gitignore` | Text conventions, line endings, and local-file exclusions |
| `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md` | Contribution and private reporting routes |
| `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md` | Project license, asset attribution, and third-party terms |

The current database baseline does not automatically convert databases from earlier development builds. A baseline change must include a one-time conversion for retained development data: create a consistent backup, import active records into a database created from the new schema, compare every retained record, and verify integrity and foreign keys before replacement. Preserve the original backup and verify a real desktop startup with the converted data. Fresh test databases alone do not establish that the development environment remains usable. Do not delete retained data or change migration checksums to bypass validation.

Tauri regenerates `apps/desktop/src-tauri/gen/schemas/` during its build. These editor schemas and resolved permission inventories are ignored build output. Edit `capabilities/default.json` or `tauri.conf.json` to change application permissions; do not edit generated copies.

### Engineering rules

- Keep domain logic out of React views and thin Tauri command adapters.
- Put storage, runtime, module, SteamCMD, and Windows behavior in their existing crates.
- Replace superseded paths completely. Do not leave parallel old, new, temp, backup, or unexplained compatibility implementations.
- Prefer self-explanatory code. Comments should document a constraint, invariant, trade-off, or non-obvious reason rather than restating the code.
- Give asynchronous work a clear owner, bound, cancellation path, timeout, error path, and cleanup behavior.
- Treat file, network, process, database, environment, and UI input as untrusted at the relevant boundary.
- Add normal, boundary, failure, and concurrency coverage when the changed behavior requires it.
- Preserve unrelated working-tree changes and avoid repository-wide formatting in a focused pull request.

### Installation lifecycle

Game-file operations own a lifecycle lease for their module and concrete program directories. Different modules can download, archive or restore concurrently when their directories do not overlap; the same module, aliased directories and parent/child directories remain coordinated. SteamCMD has a separate runtime lease retained through native execution and Workshop cache consumption: operations sharing that runtime remain serialized, while unrelated local archive work and direct downloads can proceed. Keep the order instance mutation, game lifecycle, then SteamCMD lifecycle, and retain all leases until owned workers finish. Re-read program ownership after acquisition before changing files.

Archive inventory operations still admit one mutation at a time. Archive and restore perform large file work outside SQLite write transactions, while durable journals reserve recovery paths and short transactions publish metadata. Permanent archive cleanup retains its existing transaction boundary. A responsive interface or a queued task is not evidence that two conflicting filesystem operations may overlap.

Program seeding uses live library or private installations first. It checks registered library variants and independent instance copies before falling back to archives or official acquisition. Each candidate is copied and hashed into its own staging directory; a damaged earlier candidate cannot hide a complete later source. Candidates never mix package versions. Selection retains only one staging tree: after a usable next manifest is found, the previous partial copy is discarded before copying the next candidate. A complete live source avoids archive admission and the installer. Archived seeds hold a read lease only for the selected archive; restore and purge take its matching write lease. The copier retains that lease until its worker finishes, including when the caller stops waiting. Unrelated archives can still change. Creation inventory checks existing installations directly and queues only the archive fallback. Desktop retirement protects the affected instance while other instance, installation and system refreshes continue; a late bootstrap response must preserve collections changed since that request began.

Ordinary backup restore and ARK cluster snapshot, restore and recovery copy, hash and dispose of files outside SQLite write transactions. A short publication fence revalidates registered ownership and scope roots before directory moves. The instance leases and backup coordinator remain owned until publication or compensation finishes. ARK's durable commit is authoritative during recovery, including failures while releasing the database fence or disposing of old copies.

Instance configuration preparation retains its instance lease while copying and verifying Workshop payloads outside the SQLite write transaction. Publication rechecks current ownership, runtime state, paths and ports, then commits the small activation files and database records together. Package payloads retain their existing independent publication/ownership records; a rejected configuration publication rolls back its activation files and database changes without removing separately installed packages.

Windrose's first start runs the native server to generate its own identity and world, waiting up to 120 seconds for matching server/world IDs. Only after confirming normal native exit and complete process-tree shutdown does LanGame bind the generated world, apply the instance configuration and ports through the existing materialization/updater path, rebuild the launch plan and start normally. An `Error` instance can resume when it has no active run and desktop reconciliation confirms no surviving managed process. The recovery journal contains the original configuration and is fully written and synced before atomic publication without replacement. Cancellation or failure preserves generated native data and the original configuration backup; if no native output exists, the unchanged original file can be restored. Unconfirmed process cleanup retains ownership and recovery state instead of permitting the normal start. These are implementation and recovery guarantees, not a claim that native first-world generation has passed real-server acceptance.

Modules declare native configuration and identity files outside their save trees in `storage.retained_paths`. Each entry is a literal installation-relative file or directory; uninstall retains its exact bytes even without an instance record. Executables, mod payloads, logs and caches do not belong in this declaration. Dragonwilds creation separately excludes its native identity configuration so a new private instance cannot inherit another server's GUID or administrator grants; subsequent starts preserve that instance's own identity.

Server-file uninstall preserves declared save trees, instance configuration and backups at their existing paths while removing the server payload. Overlapping preservation paths are moved once. A confirmed uncommitted state write restores the detached installation; an unreadable commit outcome retains the recovery journal and staged files until an explicit retry. Roots containing only retained data use `.langame-uninstalled` so refresh reports not installed; a verified executable takes precedence after reinstall. ZIP and SteamCMD reinstallation into these marked roots acquire the new package in an independent staging directory, then merge retained files before publication and keep the old tree until verification succeeds. Before merging retained data, the installer awaits an original-package inventory callback against the isolated verified payload. The desktop records its initial and filtered program inventories there, then retains only matching inventories after publication; it never treats merged personal files as original programs. Callback failure or conflicting retained inventory metadata prevents publication. SteamCMD cannot overwrite retained configuration with depot defaults during this operation. Unresolved save boundaries, links in preservation paths and unreadable data stop removal.

Archive and delete are separate storage transactions, both refusing active or starting/stopping instances. Archive retains personal data and backups; delete removes the managed instance directory without creating an archive. Archive preserves the library installation; after the last permanent deletion, extra unused libraries are reclaimed as described below. Explicitly owned data inside an exclusively used installation is archived or deleted with its instance, while unrelated external saves remain. Modules opt into `storage.program_sharing = "shared"` only when configuration, saves, logs and mutable Mods are isolated from program files. Minecraft vanilla currently opts in; other modules remain independent. ARK ASE/ASA separate their world directory and main log, but native configuration, access lists and Mods still write to fixed installation paths, so neither is approved for sharing. Shared runtime directories contain an identity-bound reference to one library installation. Updates retain the actual program directory's installation lifecycle lock and reject running references. Library and manual maintenance also reject pending starts; a start that already owns this lock may update before other queued starts acquire it. Shared-instance Mod operations detach before writing program files; exclusive references keep using their retained installation. Independent instances update their own registered program directory instead of inheriting library changes. The persisted `program_update.policy` defaults to `automatic`; `pinned` preserves the current program and blocks all update entry points until explicitly changed. Changing policy requires an idle instance and the program lifecycle lock. Every automatic start checks its supported source without the previous 24-hour timestamp shortcut, then renders configuration after updating. A read-only Minecraft release/JAR/Java check allows already-current shared programs to launch without mutation. Required updates blocked by running references or archives fail startup. Whole-directory replacement installers remain manual. Startup, library and instance maintenance use the same baseline finalization: retain matching same-version inventories, record verified isolated payloads before publication, and revoke stale inventories rather than treating operator files as original program files.

An unused verified installation can be bound exclusively to the first independent instance without moving or copying its program files. Its library registration remains independently managed; `runtime_mode=independent` with a library installation requires exactly one instance reference and an identity-bound exclusive marker. The installation retains a durable usage record before native configuration is written, including interrupted creation. Official acquisition records a complete initial inventory for first-use verification and a filtered program allowlist for subsequent independent copies. A previously used original library can be bound again after its former instance directory is gone, all live references and archive reservations are released, and its identity records remain valid. Reuse requires every mandatory program file and any surviving shipped default to match the recorded baseline; missing defaults and empty ordinary directories are allowed. Unknown files, modified defaults and links prevent in-place reuse without being removed. Previews inspect inventory and surviving defaults, and creation verifies mandatory program files under the lifecycle lease, reusing unchanged verified file identities. Explicitly retained repair libraries remain copy sources. Additional instances never inherit unknown files, saves, settings or Mods; local import from a previously used installation is refused. Creation always selects verified original files. Missing or changed program sources trigger one automatic official acquisition, reusing only allowlisted files whose hashes match. Rebuilding a missing original library restores and retains that baseline library. Creation repairs publish a persistent library under the games root, including repairs seeded entirely from another local instance. The library retains its own registration and a durable source-purpose marker before publication; independent instances copy its verified program files into their own `runtime`. Deleting every instance preserves this source for later creation. Shared-capable modules may still reference the library explicitly. The first repair therefore retains both a library program and an independent instance copy; later creations need only their own copies. Original library directories, existing instances, modifications and archive dependencies are not moved, rebound or overwritten. The operation retains its lifecycle lease through verification, acquisition, registration and instance creation, reports progress and joins cancellation cleanup. A previous incomplete acquisition is preserved on retry, including its registration; only its verifiable files can seed another fresh acquisition. Revalidating the same installed version retains an existing intact baseline; version changes or failed hashes invalidate it. No hardlinks or generic filesystem sharing are used.

First-use creation reuses installation-time SHA-256 results only when each file still matches its recorded identity, length, modification time, change time and expected digest. Windows supports this fast path on local NTFS/ReFS volumes, with read handles denying concurrent writers and replacements. Missing or stale optional records trigger content verification for the affected files; unsupported filesystems and non-Windows hosts use full content checks. Cache records are optional and omitted if they would exceed the manifest size limit. A freshly published staged payload uses the same incremental check, so merging retained settings cannot certify them as original defaults. Explicit validation, archive dependencies and deletion protection still verify content; overlapping clean/initial checks reuse only results from the same verification operation. Independent instance copies hash and compare each payload in their single copy stream, validate the complete selected file/directory set, and write the target inventory from the frozen source plan before publication. They do not copy mutable source inventory metadata or add unlisted files.

Library uninstall enumerates every registered library installation for the module, including prior repair and acquisition roots; instance-owned installations are excluded. Live-instance and archive references retain their required programs. After the last instance is permanently deleted, cleanup retains a sole eligible installed library without reading its program contents; this is a retention decision, not a health check, and later creation still verifies its source. With multiple candidates, cleanup verifies candidate programs and prioritizes the newest intact library marked as a retained source, falling back to another intact library; if none has a verified baseline, it conservatively retains the canonical installed library or the oldest usable registered source. It removes only hash-confirmed original files and recognized manager metadata from extra libraries. Unknown or modified files remain at their original paths; an unverified package or a protected executable retains the whole installation with an explicit reason. Data-only directories are marked uninstalled and do not count as installed program sets. A NotInstalled record alone cannot enable creation or implicit repair: both preview and the locked creation entry require another live or archived program source, otherwise an explicit installation is required. Removal uses a durable SQLite journal and identity-checked sibling staging: database publication and the committed removal phase are atomic, interrupted uncommitted work rolls back, and committed work only finishes cleanup. Recovery runs only on explicit deletion or uninstall, never on application startup. The result includes removed roots, retained data paths and reasons for retained programs.

The creation-space estimate counts logical bytes of allowlisted program files after the current copy exclusions, excluding old saves, Mods and custom configuration. It inspects metadata rather than rehashing the full package; creation still verifies payloads under its lease. An unconfirmed source, missing allowlist or incomplete payload yields an unknown estimate. An installation's displayed size remains its actual whole-directory logical size, including personal data. Neither value is physical disk allocation or guaranteed reclaimable space; the storage allocation scan separately deduplicates hardlinked files by file identity.

Archive snapshots preserve exact program dependencies and actual personal-file bytes. For an exclusive external installation, the durable manifest distinguishes stored modified or unknown files from original program files retained at their source. Only explicitly owned native configuration and saves may be removed from that installation; unknown files remain. Restore verifies source identity and file digests and refuses conflicting destination bytes. Missing trustworthy program inventories produce complete program archives. Updates, validation, uninstall and mutable exclusive starts check archive dependencies before changing a required source. Shared archives likewise record exact dependencies; when every program byte is stored they are complete archives. A complete archive may release its source only after its saved payload is verified, and can recreate a missing original program directory. Conflicting replacement directories are retained and rejected. Existing v1/v2 archives retain their recovery paths. Archive and delete remain distinct durable operations; partial deletion requires explicit retry, and startup recovery does not resume destructive cleanup.

The ignored desktop test `native_package_lifecycle` accepts `LANGAME_NATIVE_MODULE_ID` and `LANGAME_NATIVE_PACKAGE_ROOT` for one existing package. It copies files into owned temporary storage, excludes declared worlds, verifies program ownership, and runs production start, the module's `smoke.toml` readiness probes, stop, persisted configuration readback and restart with fresh readiness baselines. Backup, library uninstall, archive/restore and permanent instance deletion check the corresponding retained or removed bytes; shared references must prevent library uninstall. Servers that produce no save data without players report that observation without claiming save generation or backup coverage. The default `LANGAME_NATIVE_ACQUISITION=local_package` uses storage creation and does not certify the supplied package's provenance. `official_acquisition` explicitly acquires an official package with the production installer in a fresh disposable directory before invoking the desktop creation coordinator; it rejects `LANGAME_NATIVE_STEAMCMD_ROOT` overrides. `LANGAME_NATIVE_INSTANCE_MODE=second_private` requires official acquisition and creates two independent installations. It corrupts a declared entry in the exclusively used first installation and adds unknown-Mod and old-save sentinels. Production creation must automatically acquire missing original bytes in a retained library and copy its verified program into the second instance's runtime, while preserving the first installation, including its intentionally changed byte. The second installation must contain verified original files without inheriting either sentinel. Final deletion checks that the original library and repaired source remain, while the second instance's program and registration are released. The repaired library remains reusable after all instance copies are deleted. `shared_pair` requires a shared-capable module and verifies one program root with isolated configuration, saves and ports; its peer remains unstarted, so this does not establish simultaneous operation. No mode establishes graphical-interface or client-join acceptance.

Use a non-elevated Windows process and the selector `native_package_lifecycle -- --ignored --nocapture --test-threads=1`. For a serial batch, replace the single-module variables with `LANGAME_NATIVE_MODULE_IDS` (comma-separated IDs, at most 32) and `LANGAME_NATIVE_PACKAGES_ROOT`; each source comes from the module's declared installation subdirectory. Each case owns and cleans its own fixture, and the first failure stops the batch. Minecraft requires the operator's prior `LANGAME_SMOKE_MINECRAFT_EULA_ACCEPTED=true`; DST accepts explicit `LANGAME_NATIVE_DST_OFFLINE_LAN=true` for the offline LAN variant. SCUM alone permits an operator-authorized elevated process with `LANGAME_NATIVE_ALLOW_ELEVATED_SCUM=true`; the fixture verifies and removes only its exact unchanged firewall rules after shutdown. Other elevated fixtures are rejected. ProgramData and user-data environment paths are temporary; native KnownFolder APIs are not an environment-variable sandbox, so game-specific save paths remain necessary. Copying is bounded to 64 GiB and 1,200 seconds by default; `LANGAME_NATIVE_COPY_MAX_BYTES` and `LANGAME_NATIVE_COPY_TIMEOUT_SECONDS` allow bounded overrides. Copying and instance creation report separate elapsed times.

Instance creation stays visibly pending during program preparation, including path checks, content verification when required, and program copying, even when the operator leaves and returns to the library. Duplicate submissions for that game are rejected until the request settles. Explicit runtime-service shutdown closes admission and reserves a storage lease under stable paths for its server stop work. It cancels queued/preparing creation and installation work while running server stops under their instance locks; unrelated storage work does not block all game save commands. Backup archives begin after all server stop attempts. Shutdown exclusivity is acquired only after every storage lease settles, with a 120-second drain limit. Closing the main window alone does not request this shutdown. Creation checks cancellation between metadata entries or copy/hash blocks and before its database transaction; pending directories are removed on cancellation. Once a filesystem/database transaction has begun, it reaches commit or rollback before releasing its lease. A drain timeout identifies remaining operations; update shutdown aborts, while final tray shutdown retains its independent deadline.

SteamCMD availability uses `SteamCmdStatus.ready`, not executable presence. Preparation must observe a fresh Steam console and successful API initialization, successful exit, and an empty owned process tree. A managed readiness record is invalidated before verification and bound to the current runtime files; failed or cancelled initialization never creates one.

Steam server installation, update, validation and Workshop downloads require an already verified SteamCMD. They do not install or repair this dependency implicitly. The library disables dependent installation controls until SteamCMD is ready, with a tooltip directing the operator to the System page's explicit check/install action. Minecraft Java and direct-download installers remain independent. The backend repeats the dependency check before changing program files; the interface refreshes availability after installation operations.

Managed Windows SteamCMD preparation fetches Valve's current signed win64 manifest through the equivalent official sources in `app-network/official-sources.json`. Missing package-cache entries are downloaded with size/SHA256 verification and atomic publication; valid cache entries are reused. A short-lived loopback endpoint supplies that same manifest to native SteamCMD during preparation and subsequent scripts, avoiding another uncontrolled updater download. Native SteamCMD still applies and verifies the update; no `.installed` file or persistent host override is fabricated. A missing or damaged bootstrapper is seeded from the manifest's verified official ZIP. Download progress reports the aggregate missing payload, including the bootstrapper ZIP when needed.

The ignored `steamcmd_update_cache::tests::live_direct_install_cache_reuse_and_cancel` test requires `NO_PROXY=*` and `LANGAME_STEAMCMD_LIVE_ROOT` set to an unused absolute diagnostic directory. It downloads and executes official SteamCMD, checks fresh readiness and a second run without package downloads, and verifies cancellation leaves no staging file or ready marker.

SteamCMD preparation and server installation/update/validation expose cancellation through their operation IDs. A stop acknowledgement only sets `cancel_requested`: the provider retains its operation lock until child processes, output readers, and pending file work have settled. Only confirmed cleanup produces `Cancelled`; cleanup failure remains `Failed`. The desktop keeps active jobs visible and offers the stop action in the shared activity bar. Archive/JRE staging and SteamCMD reinstallation into marked retained-data roots restore previous files on cancellation. Ordinary native Steam installations and updates preserve Steam's partial files for the next run and cannot undo files already updated.

The separate ignored selector `native_game_tools -- --ignored --nocapture --test-threads=1` uses the same isolated local-package prerequisites, but only runs production creation, start, readiness, game-tool effects and confirmed shutdown/cleanup. Export the current frontend commands and declared maintenance actions with `node apps/desktop/scripts/export_native_gm_cases.cjs <absolute-output-outside-repository.json>`, then set `LANGAME_NATIVE_GM_TOOLS=true` and `LANGAME_NATIVE_GM_COMMANDS_FILE` to that output. The probe checks fresh responses, logs, persisted save changes or native DST world/entity state as applicable. DST creates two native player entities per shard to check which inventory changes; this does not establish client-join acceptance. ARK world commands establish RCON response coverage only; rewards to a joined player's inventory still require client observation. This selector does not replace the archive/restore lifecycle test.

For a focused native startup/shutdown comparison, `native_start_stop -- --ignored --nocapture --test-threads=1` uses the same isolated local package and production readiness/stop paths without the archive or restart phases. `LANGAME_NATIVE_SETTINGS_JSON` can overlay a JSON object of at most 64 KiB onto this disposable instance's smoke settings; normal settings validation and fixture isolation still apply. `LANGAME_NATIVE_OBSERVE_SECONDS` permits 0–120 seconds of observation. These backend probes do not certify the desktop display, long-running stability, or client joins.

The Forest's native `DllModLoader` hosts an embedded stdin bridge limited to `help`, `status`, `save`, and `shutdown`; it does not provide player moderation or bypass network admin authentication. The Windows build uses the platform .NET Framework C# compiler and BCL only, without private game/Unity build dependencies. Preparation verifies the game fingerprint and independent program ownership and refuses conflicting user DLLs. Save runs on the Unity main thread and requires a new nonempty checkpoint file identity; shutdown saves first, cancels and joins the owned stdin reader, then invokes the native shutdown coroutine, with success still requiring the complete owned process tree to exit. Its native save-folder setting needs a trailing separator. Nonempty or non-plain old `savesMultiplayer` / `savesSinglePlayer` directories block startup with recovery instructions. See [bridge maintenance and checks](../modules/theforest/control/README.md).

On Windows, `build.rs` invokes `scripts/build_ark_tools.ps1` to build the ARK creature extension in Cargo's `OUT_DIR`. SDK sources, import libraries and loader sources use fixed upstream revisions and SHA-256 checksums; binaries are not written into the repository. The first build needs network access and the existing MSVC C++ toolchain. Source provenance and third-party licenses are in `modules/ark-tools/`. Installation downloads the pinned runtime framework and deploys the embedded plugin; end users do not need a compiler. Installation and spawning are available only through local desktop IPC. The runtime retains the instance lock through spawning and independent ID read-back; the HTTP management interface does not expose these writes.

Adding `LANGAME_NATIVE_ARK_CREATURES=true` invokes the production extension installer before starting each isolated ASE/ASA instance. After startup, it invokes production spawning to create a level 37 wild Dodo and a level 150 wild Rex, verifies native entity identity, class and level, checks duplicate request IDs do not spawn twice, and requires native rejection of unknown classes and absent online owners. This option includes ASA's matching-symbol download, so obtain the operator's explicit consent first. An empty-server fixture does not establish taming ownership for a real online player or client rendering.

### Multi-instance and cluster transactions

Directory isolation applies to instances of the same or different games; it is not a sandbox or separate OS identity. Module port groups are allocation units during creation and startup remapping. ASE declares game/peer offsets 0/1; a busy derived port moves the complete pair. Keep offset groups, mixed-protocol groups and disabled-port contracts covered by deterministic probe tests. ASA is not assigned ASE's contract by inference.

ARK grouping uses edition, Cluster ID, canonical directory identity and a sorted member snapshot. Group operations retain ordinary instance lifecycle checks and return individual results. The local desktop's cluster snapshot transaction locks ARK creation and configuration, reserves database ownership, and requires every member to be stopped. Its explicit boundary is each member's configuration and native Saved tree plus the user-confirmed exclusive transfer root; it does not guess a native subdirectory or restore another cluster's data.

Within one ASE/ASA instance, `additional_maps` defines up to 15 additional map processes that share its existing installation, configuration, access lists and Mods. Each enabled map gets stable registered ports, its own native log, world directory and LanGameCMD target. Paused maps retain their endpoints; removing configuration retains saved worlds. Ordinary backups own the whole native Saved tree; historical primary-only backups restore into the primary world's directory. Runtime health and startup listener checks verify each enabled map's owned process. Opt-in `native_package_lifecycle` acceptance supports `LANGAME_NATIVE_ARK_MAPS=ScorchedEarth_P` for ASE or `ScorchedEarth_WP` for ASA, and checks all map endpoints, fresh logs, saved directories and group cleanup in disposable storage. This does not enable program sharing between independent instances or prove in-game client transfers.

Whole-configuration restore requires snapshot name, ports, bind address and autostart mirrors to match the current database registration. Reject mismatch during preflight; do not rely on a later startup regeneration to repair conflicting configuration. Public-API tests must exercise real database admission and verify settings, saves and registration after read-back.

Snapshots use bounded manifests, SHA256 verification, plain-directory checks and actual volume free-space checks. The current bounds are 128 ARK instances, 200,000 entries, 64 directory levels, 1 TiB of data, 1,000 snapshots per cluster root, and 512 MiB of remaining capacity headroom. Native Windows capacity queries must retain extended-length/UNC path handling. Restore first publishes a protection snapshot, stages replacements beside their targets, and journals publication. Member-root pointers preserve recovery identity even when a configuration directory is between renames. Explicit interruption recovery rolls back uncommitted publication or completes committed cleanup; startup must reject pending transactions before reading or regenerating configuration.

Runtime resource policy is persisted separately from its applied run snapshot. CPU limits use whole-host percentage; memory limits use aggregate Windows Job committed bytes for every instance process, including both DST shards. Memory admission accounts for active configured budgets, pending launches and host reserve. Settings changes require a stopped instance and apply at the next launch; a saved value is not proof that an existing Job changed.

### Background runtime ownership

On Windows the current-user runtime process owns server jobs, storage mutations and recovery workers; the interface reconnects through authenticated local IPC. Closing the main window hides it and keeps the interface process and tray alive. Tray activation or a second launch restores that window; automatic WebView recreation preserves its requested visibility. Interface loss does not transfer or terminate runtime ownership. Explicit tray exit hands final shutdown ownership to an independent native helper, then exits the interface without waiting for saving or stop completion. The backend uses the same save/stop and instance backup policy as manual stopping, and ends when cleanup completes or its fixed final deadline expires. Application update downloads finish before requesting shutdown, and installation still requires a completed shutdown receipt and successful runtime exit. This is a user-session process, not a Windows system service or a promise of survival across sign-out or restart.

The managed development launcher retains its build-target lease and frontend process for the lifetime of that runtime. Reopening uses the running runtime's original executable and target. If its launcher or frontend is unavailable, report that state without killing the service, switching targets or starting a competing runtime. The portable source commands in the README remain unchanged.

### Player counts

`runtime.player_count_source` explicitly selects `player_query` (the default UDP query contract) or `player_list` (the declared online-player source). Squad and HumanitZ use their authenticated `player_list`: their instance overview and System totals share the same complete roster as the player center. Only a fresh, complete, non-truncated `ready` result can supply a count; failed authentication, disabled RCON, changed process identity and incomplete responses remain unknown. Automatic polling shares in-flight requests and the module's monotonic refresh interval, including failed attempts. HumanitZ retains its disabled RCON default; operators must configure and enable it to query players.

System totals show unknown when no running instance was successfully queried, and a lower bound when only some running instances were queried. A successful empty roster is zero. Configured capacity remains a separate value. Selecting a verified roster source does not establish that native A2S works or that all server modes lack A2S support. The opt-in native lifecycle fixture compares the production roster count, instance overview and global total for `player_list` modules against the same active run.

An isolated HumanitZ build 23914958 comparison on 2026-09-21 reproduced no A2S replies with the shipped EOS session provider. Changing only `OnlineSubsystem.DefaultPlatformService` to `Steam` in a disposable copy produced valid A2S_INFO replies on the same query port. This identifies a native session-provider dependency in that build; it does not justify changing the production provider, which also controls multiplayer discovery. Keep the native provider unchanged and use the authenticated roster for manager counts.

On 2026-09-21, Squad build 25149748 loaded a playable game mode and its default RedpointEOS session, with a complete authenticated roster. Windows socket tracing confirmed delivery of INFO requests to the query socket, with no receive calls or replies; game-port and independent UDP controls received normally. Socket binding alone does not establish native query readiness. Isolated fixtures must preserve the full world/session/roster checks and the installed map-voting defaults.

The isolated Squad Steam-provider override failed native autologin and session creation: Steam rejected mismatched `bUsesPresence` and `bUseLobbiesIfAvailable` before starting its advertised-server task. This override is not a compatible fix and must not replace the default provider or justify weakening readiness checks.

### UI and translations

Follow the shared [desktop typography and window layout contract](desktop-ui.md)
when changing interface text, controls, responsive layouts or native window bounds.

Do not repeat an active navigation label as a page heading. Use cards only for content with an independent object, state, or action boundary. Cover loading, empty, error, disabled, in-progress, success, and permission-denied states when applicable.

Keep action confirmations in the application. Use `InlineConfirmAction` at the triggering button for destructive actions, with the affected object and consequences in the message. Bind the review to the target and relevant inputs, cancel it when they change, and return the operation promise to prevent duplicate submission. Do not use browser `window.confirm` or `window.alert`; complex operation previews use the existing application review UI.

Every user-visible string must have English and Simplified Chinese coverage. Update source catalogs or generators, not generated catalog chunks, and run:

~~~powershell
npm --prefix apps/desktop run verify:i18n
npm --prefix apps/desktop run verify:i18n-readable
~~~

### Game modules and evidence

Instance autostart and save policies are managed on the Maintenance page. `settings/save-policy.ts` assigns native autosave fields, backup rotation fields, and the managed Unturned save interval to that page; configuration, mod, and player pages do not edit them again. Policy changes retain schema types, bounds, and cross-field validation, and merge only the changed fields. A successful save and a failed refresh are reported separately. Native settings are written to the game's files or launch arguments for the next start; stop-time backup policies apply after saving.

Every game shows an Autosave section. Games with a configurable policy expose its frequency or native switch; others explain the actual save mechanism and limits. Barotrauma saves at campaign stages, RimWorld receives player saves from clients, and Squad has no recoverable world save. Unturned has no native autosave: `commands_managed_save.rs` periodically sends the module-declared `Save` command to a registered live process. Its interval defaults to 300 seconds; 0 disables it, and changes apply to running instances. Tasks are tied to process identity, storage context, and application shutdown. Pending writes cannot overlap, and successful command delivery does not prove the game has finished writing its save.

`verify:configuration-workspace` covers policy ownership and frontend lifecycle. The `app-storage` and `app-runtime` `save_policy` tests cover persistence and native output for 32 modules. Desktop `managed_save` tests use a controlled stdin process to cover due saves, disabling, restarts, lost ownership, and late acknowledgements; they do not replace native game-save smoke tests.

Don't Starve Together world-setting icons use the installed server's local image
atlases. Valid atlases are retained under
`<app-data-root>/cache/dontstarve-configuration-icons/data/images/` so the icons
remain available when the server files are absent. The cache contains only
`worldgen_customization.{xml,tex}` and `worldsettings_customization.{xml,tex}`;
valid installed atlases take priority and refresh the cache. The desktop decodes
them on a bounded worker and sends PNG data URLs to the settings page. It does
not download artwork or write into the game installation. If neither source is
available, the page shows a notice and a retry action; text controls remain
usable. The source distribution and browser mock do not contain game artwork.

Plain Lua data tables for Don't Starve Together world configuration stay synchronized with guided fields. Reads show effective native values; edits change only the selected value and preserve other content and comments. Both frontend and backend use bounded static parsing with limits on input size, nesting, and entry count; neither executes user scripts. Files containing program logic remain in the advanced editor, with the corresponding guided fields read-only. Additional override lines are retained but inactive while a full script is in use. Disabling caves pauses that shard without discarding world or mod settings.

Read [Game Integration Validation](game-integration-validation.md) before adding a game or changing its configuration, launch, lifecycle, port, query, or player-management contract.

- Base support claims on authoritative documentation, a lawfully obtainable Windows dedicated-server package, or a reproducible installation probe.
- Map every modeled native setting and deliberate exclusion in `config-sources.toml`.
- Keep deterministic, sanitized acceptance fixtures under `modules/<module-id>/config-fixtures/`.
- Do not infer server behavior from a game client, another game, marketing copy, or an unverified community guide.
- Do not commit proprietary binaries, game assets, private logs, machine-specific paths, or secrets as evidence.

The configuration source ledger and acceptance records are generated outputs. Change their module sources or generator, then regenerate them:

~~~powershell
python -B scripts/verify_game_config_provenance.py --write
python -B scripts/verify_game_config_acceptance.py --require-all --write
~~~

Do not edit `docs/game-config-source-ledger.md`, `docs/game-config-source-ledger.csv`, or `docs/game-config-acceptance/*.md` by hand.

### Dependencies

Keep dependency updates separate unless they are required by the same fix. For each update, review the upstream release and migration notes, runtime and toolchain compatibility, license changes, supply-chain impact, and the resulting lock-file diff. A newer version alone is not sufficient evidence that an update is safe.

### Verification

Run the narrowest relevant checks first, then expand according to the affected boundaries. The portable repository baseline is:

~~~powershell
python -B -m unittest discover -s scripts/tests -p "test_*.py"
npm --prefix apps/desktop run verify
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
~~~

Game configuration changes must also run the check-mode generators:

~~~powershell
python -B scripts/verify_game_config_provenance.py --check
python -B scripts/verify_module_setting_coverage.py
python -B scripts/verify_game_config_acceptance.py --require-all --check
python -B scripts/verify_library_media.py
~~~

Shared dedicated-server smoke infrastructure is not part of the public repository because it depends on controlled storage and lifecycle policy. The opt-in package-copy test above runs locally against an operator-provided package. Report deterministic checks separately from native results and identify the sanitized environment used.

For process, console, log, scheduler and Web lifecycle changes, run the [synthetic runtime reliability suite](runtime-reliability.md#english). It records bounded process/resource and fault-recovery evidence without starting real game instances.

### Source distribution

The first public source release starts from one reviewed root commit. Before replacing development history, retain and verify a repository-external recovery backup. The local and public master branches use the same reviewed source commit; keep the backup private and never push its old references. Later releases record ordinary commits rather than resetting history again.

Review the exact release source and retain its license, brand rules, third-party notices, and required build inputs. Before preparing the snapshot, run the current-source checks:

~~~powershell
python -B scripts/verify_no_tracked_secrets.py
python -B scripts/verify_open_source_boundary.py
~~~

Current-source scans fail when candidate files cannot be read or escape the repository boundary; skipped content must never be reported as verified. A snapshot without history still requires checking the files it contains.

Preview the existing snapshot exporter:

~~~powershell
python -B scripts/export_public_snapshot.py --verbose
~~~

The preview examines tracked working-tree files; it does not include untracked files and is not the final immutable release. After reviewing and committing all intended source changes, including new files, use a clean checkout and a new output directory outside the repository. Do not discard unfinished work just to satisfy the clean-worktree requirement:

~~~powershell
python -B scripts/export_public_snapshot.py --execute --output ../langame-public-source
~~~

The exporter copies the clean `HEAD` source, checks its contents and entry-point references, and writes a file-hash manifest. It excludes Git history, local workstation launchers, runtime data, and unapproved assets. The manifest retains the source commit ID for traceability, not the commit objects or their history. The exporter does not modify repository history, initialize a Git repository in the output directory, commit, push, or publish. An export from an earlier commit does not include later uncommitted changes.

Export-time secret, product-boundary, and entry-point checks do not replace the complete project license checks, build, or installer verification. Review and build the exported candidate in a clean external environment before separately authorizing publication. Use the reviewed snapshot for the first public commit, then maintain the same master history locally and on the public remote.

Existing CI history verification remains enabled; in the public repository it checks that repository's public history, without importing private commits. History verification also remains available for internal checks. Publishing any old internal history requires a separate scope decision and the current-source checks above plus:

~~~powershell
python -B scripts/verify_open_source_history.py
~~~

A clean working tree does not remove material from earlier commits. History findings identify the commit, file, line, and rule without printing the matched value. Resolve findings before publishing that history; do not bypass the scan or treat a current-file deletion as remediation. History verification requires a complete clone, rejects shallow history, and reads original Git objects rather than local `git replace` substitutions, including commit and annotated-tag messages. Message findings use `git-metadata/` locations. Separately review whether Git author and committer names and email addresses are intended to be public.

## 简体中文

按[从源码运行](../README.zh-CN.md#从源码运行)准备工具链并启动桌面。Issue 与 Pull Request 流程见[贡献指南](../CONTRIBUTING.md#简体中文)。

### 仓库结构

| 路径 | 职责 |
| --- | --- |
| `apps/desktop/` | React 界面、翻译、前端检查与 Tauri 命令适配层 |
| `crates/` | 领域类型、存储、运行时、模块接入、SteamCMD 与 Windows 支持 |
| `modules/` | 游戏清单、设置 Schema、原生模板与验收夹具 |
| `migrations/` | 工作区数据库结构 |
| `docs/` | 操作指南、接入契约、协议与生成的证据记录 |
| `scripts/` | 可移植校验、生成与源码分发工具 |
| `.github/` | Issue 表单、Pull Request 模板与 CI 工作流 |

根目录文件分别承担入口或工具配置职责：

| 文件 | 用途 |
| --- | --- |
| `README.md`、`README.zh-CN.md` | 产品介绍与可移植的安装命令 |
| `Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml` | 工作区依赖、可复现依赖解析与 Rust 工具链 |
| `.editorconfig`、`.gitattributes`、`.gitignore` | 文本规范、换行符与本地文件排除规则 |
| `CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、`SECURITY.md` | 贡献流程与私下报告渠道 |
| `LICENSE`、`NOTICE`、`THIRD_PARTY_NOTICES.md` | 项目许可证、资产署名与第三方条款 |

当前数据库基线不自动转换早期开发构建的数据库。变更基线时，必须为需要保留的开发数据完成一次性转换：先创建一致性备份，将有效记录导入按新结构创建的数据库，逐条比对保留记录，并在替换前验证完整性与外键。保留原始备份，并使用转换后的数据验证真实桌面启动；仅验证新建测试数据库，不能说明现有开发环境仍然可用。不得删除需要保留的数据，或修改迁移校验和来绕过验证。

Tauri 构建会重新生成 `apps/desktop/src-tauri/gen/schemas/`。这些编辑器 Schema 和解析后的权限清单属于已忽略的构建产物。应用权限应修改 `capabilities/default.json` 或 `tauri.conf.json`，不要修改生成副本。

### 工程约定

- 领域逻辑不得堆入 React 视图或轻量 Tauri 命令适配层。
- 存储、运行时、模块、SteamCMD 和 Windows 平台逻辑应放在现有对应 crate 中。
- 完整替换已废弃路径，不得保留并行的 old、new、temp、备份或无说明兼容实现。
- 代码应优先自解释。注释用于说明约束、不变量、取舍或不明显的设计原因，不应复述代码表面行为。
- 异步任务必须有明确所有者、并发边界、取消路径、超时、错误处理和资源清理。
- 在对应边界将文件、网络、进程、数据库、环境变量和界面输入视为不可信数据。
- 根据改动补充正常、边界、异常和并发测试。
- 保留工作树中与本次任务无关的改动，目标单一的 Pull Request 不得执行全仓格式化。

### 安装生命周期

游戏文件操作按游戏模块及实际程序目录持有生命周期租约。不同模块在目录不重叠时可以同时下载、归档或还原；同模块、目录别名及父子目录仍受协调。SteamCMD 使用独立的运行环境租约，并持有到原生执行及创意工坊缓存读取完成：共用该环境的操作仍串行执行，无关的本地归档或直接下载可以继续。加锁顺序为实例变更、游戏生命周期、SteamCMD 生命周期；后台工作完成前不得释放租约，取得租约后须重新检查程序归属再修改文件。

归档清单操作仍一次只接纳一项变更。归档和还原的大文件操作放在 SQLite 写事务之外，由持久化事务记录预留恢复路径，再用短事务发布元数据。永久清理保留原有事务边界。界面可响应或任务显示排队，并不代表有冲突的文件操作可以重叠执行。

程序种子优先读取程序库或私有安装。先检查已登记的库变体和独立实例副本，再回退到归档或官方获取。各候选在独立暂存目录中边复制边校验，前面的损坏来源不会遮蔽后面的完整来源，不混合不同版本的程序。选择期间只保留一份暂存目录：找到下一份可用清单后，先清理前一个不完整副本，再复制当前候选。找到完整本地来源后无需进入归档队列或调用安装器。使用归档种子时仅持有选中归档的读租约，还原和清理取得对应写租约；调用方停止等待后，复制工作完成前仍保留租约。其他归档可继续变更。创建预检直接检查现有安装，仅在需要归档来源时进入清单队列。桌面归档、删除只保护受影响实例，其他实例、安装和系统刷新继续；较晚返回的启动快照不得覆盖请求开始后已经更新的集合。

普通备份还原及 ARK 集群快照、还原和恢复，将文件复制、摘要校验和旧副本清理放在 SQLite 写事务之外。发布时通过短事务重新验证登记归属和操作根目录，再执行目录移动。实例租约与备份协调任务持续保留到发布或补偿结束。ARK 恢复以已持久化的提交记录为准，数据库事务释放或旧副本清理失败也不得撤销已提交结果。

实例配置准备期间持续持有实例租约，将创意工坊包的复制和校验放在 SQLite 写事务之外。发布时重新校验归属、运行状态、路径和端口，再一起提交小型启用配置和数据库记录。程序包沿用独立发布及归属记录；配置发布被拒绝时回滚其启用配置和数据库变更，不删除独立安装的程序包。

Windrose 首次启动先运行原生服务器，由游戏生成自身身份和世界，等待服务器与世界 ID 匹配的上限为 120 秒。只有确认原生正常退出且完整进程树已停止后，LanGame 才绑定生成的世界，通过既有配置物化与官方更新器应用实例设置和端口，重建启动计划并正式启动。`Error` 状态的实例在没有活跃运行记录、且桌面协调确认无残留托管进程时可以恢复。恢复日志包含原始配置，完整写入并同步后才以禁止覆盖的方式原子发布。取消或失败保留已经生成的原生数据和原始配置备份；尚无原生产出时可还原未被更改的原文件。进程清理未确认时保留进程所有权及恢复状态，不进入正式启动。这些说明描述实现和恢复边界，不代表原生首次世界生成已通过实机验收。

模块通过 `storage.retained_paths` 声明存档树外的原生配置和身份文件。每项必须是相对安装目录的静态文件或目录；即使没有实例记录，卸载也保留其原始字节。程序、Mod 包、日志和缓存不应加入该声明。Dragonwilds 创建新私有实例时另外排除原生身份配置，避免继承其他服务器的 GUID 和管理员授权；后续启动仍保留该实例自己的身份。

卸载服务器文件时移除程序，模块存档、实例配置和备份保留在原路径。重叠的保留路径只移动一次；确认未提交时恢复隔离的安装目录，无法确认提交结果时保留恢复记录与暂存文件，等待明确重试。仅剩保留数据的目录使用 `.langame-uninstalled` 标记，刷新后显示未安装；重装后存在有效程序时优先识别为已安装。ZIP 和 SteamCMD 向这些标记目录重装时，先在独立暂存目录获取新包，再合并保留数据、发布并验证，成功前保留旧目录以便恢复；合并保留数据前，安装器等待桌面在经过验证的独立新包中记录完整初始清单及过滤后的程序清单；发布后只保留仍匹配的清单，不把混入的个人文件认作原版程序。记录失败或与保留清单元数据冲突时拒绝发布。SteamCMD 不会在此过程中用程序包默认配置覆盖保留配置。无法确定存档边界、保留路径包含链接或无法读取时拒绝删除。

归档与删除是独立的存储事务，均拒绝活动、启动中或停止中的实例。归档保留个人数据和备份；删除移除托管实例目录，不生成归档。归档保留库安装；最后一个实例永久删除后，按下述规则回收多余且未使用的库程序。独占安装中明确属于实例的数据随实例归档或删除，无关的外部存档仍保留。模块只有在配置、存档、日志和可变 Mod 与程序可靠分离时才声明 `storage.program_sharing = "shared"`。当前 Minecraft 原版启用共享，其他模块保留独立安装。方舟 ASE/ASA 虽已分离世界目录和主日志，但原生配置、名单与 Mods 仍写入安装内固定位置，因此尚未开放共享。共享实例的 runtime 目录保存绑定程序身份的引用。更新持有实际程序目录的安装生命周期锁，拒绝修改仍有运行实例引用的程序；商店和手动维护还拒绝待启动引用，已取得该锁的启动可先更新，再让排队中的其他启动取得锁。共享实例写程序文件的 Mod 操作先拆分；独占引用继续使用保留的安装。独立实例更新自己登记的程序目录，不自动继承库目录变化。实例设置 `program_update.policy` 默认 `automatic`，`pinned` 保留当前程序并阻止所有更新入口，需明确解除固定才能更新。策略变更要求实例闲置，并持有程序生命周期锁直到保存完成。自动启动每次检查受支持的安装源，不再复用旧的 24 小时验证时间；完成更新后再生成配置。Minecraft 通过只读发布版本、JAR 和实际 Java 运行时核验，允许已是当前版本的共享程序继续启动。必要更新被运行引用或归档依赖阻止时，启动失败并说明原因。整目录替换型安装器保持手动维护。启动、商店和实例维护共用基线收尾：同版本复核保留匹配清单，独立新包在发布前记录清单，旧版本或失配清单撤销，不把用户文件认作原版程序。

未使用过的已验证安装可由首个独立实例原位独占引用，不移动目录、不复制程序；安装仍保有独立的游戏库登记。`runtime_mode=independent` 关联库安装时，必须恰有一个实例引用，并匹配独占引用标记与安装身份。写入原生配置前持久化使用记录，创建中断也不会把残留误认为全新安装。官方获取时记录完整初始清单用于首次使用核验，并生成过滤后的程序白名单用于后续独立复制。曾使用的原库在旧实例目录已移除、实例引用和归档保留均已解除、身份记录有效时，可再次原位独占。复用要求必要程序与仍存在的原版默认文件匹配原清单，允许默认文件已正常删除及普通空目录；未知文件、修改过的默认配置和链接阻止原位复用，但不会被删除。预检检查目录清单和剩余默认文件，实际创建在生命周期锁内验证必要程序，并复用未变化的文件校验身份。明确保留为修复来源的库继续用于独立复制。新增实例不得继承未知文件、存档、配置和 Mod；已使用的安装不能通过本地导入隐式克隆。创建统一使用已验证原版；来源缺失或文件变化时自动获取一次官方程序，仅复用白名单内哈希匹配的文件。原库目录缺失时重建并保留这一份基础库。创建时的修复结果保留为游戏目录下的独立库安装，包括完全复用其他本地实例得到的修复结果。库在发布前持久记录来源用途，独立实例只将经过验证的程序复制到自己的 `runtime`；删除全部实例后仍保留这份来源，供后续创建复用。支持共享程序的模块仍可显式引用库安装。首次修复因此同时占用一份库程序和一份独立实例程序，后续创建仅增加对应实例副本。原库目录、已有实例、程序修改和归档依赖不会被搬移、改绑或覆盖。生命周期锁覆盖核验、获取、登记与创建，过程报告进度并等待取消清理完成。重试保留此前失败的获取目录及登记，只用其中可验证的文件为另一个新目录提供种子。同版本重新校验保留仍完整匹配的既有清单，版本变化或哈希失配时撤销旧清单。不使用硬链接或通用文件系统共享。

首次创建仅在文件身份、长度、修改时间、变更时间及预期摘要均匹配时，复用安装时生成的 SHA-256 结果。Windows 在本地 NTFS/ReFS 卷支持这一快检路径，检查句柄拒绝并发写入及替换。可选记录缺失或失效时，仅重新读取相关文件；不支持的文件系统及非 Windows 主机执行完整内容检查。缓存不得挤占基础清单的容量上限，超限时省略缓存。刚发布的暂存包也执行此增量检查，保留配置合并后不会被认作原版默认内容。显式校验、归档依赖及删除保护仍检查实际内容，clean/initial 重叠文件仅复用同一次核验中的结果。独立实例在唯一一次复制流中计算并比较摘要，检查所选文件和目录集合完整后，以冻结的来源计划生成目标清单，再发布实例；不复制途中变化的来源清单，也不纳入清单之外的文件。

游戏库卸载会遍历该游戏全部已登记的库安装，包括历史修复和获取目录；实例专属程序不在卸载范围内。仍被实例或归档依赖的程序保留。最后一个实例永久删除后，只有一个符合条件的已安装库时直接保留，不读取其程序内容；这只是保留决策，不代表通过完整性检查，后续创建仍须校验来源。有多个候选时，自动清理核验候选库，优先保留最新完整的持久来源，必要时回退其他完整库；所有来源均无法确认完整时，保守保留默认库目录或最早的可用登记来源。其余库目录仅移除哈希匹配的原版文件和已识别的管理器元数据，未知或修改过的文件留在原路径；缺少可信清单或可执行文件需要保留时，整套程序保留并报告原因。仅剩个人数据的目录标记为未安装，不计入已安装程序套数。仅有 NotInstalled 记录不能启用创建或隐式修复：预览与取得租约后的创建入口均要求另有现有实例或归档中的程序来源，否则必须先显式安装。移除使用持久化 SQLite 日志及文件身份校验的同级隔离目录，安装状态与清理提交阶段原子发布；未提交的中断操作回滚，已提交操作只继续清理。恢复只在明确删除或卸载时执行，应用启动不自动删除。结果返回已移除目录、保留数据路径及程序保留原因。

创建所需新增空间只估算程序白名单在应用当前复制排除规则后的逻辑字节，不计入旧存档、Mods 和自定义配置。预览读取元数据，不重新哈希整个程序包；创建时仍在租约内核验文件内容。无法确认来源、缺少清单或程序不完整时，估算显示未知。安装目录显示的大小仍是包含个人数据的整目录实际逻辑大小。这两项都不是物理磁盘分配量，也不保证删除后释放相同空间；存储扫描另按文件身份对硬链接的物理分配量去重。

创建实例在程序准备期间持续显示处理中，包括路径检查、需要时的内容校验，以及程序复制，离开游戏库再返回仍保留状态；当前请求结束前，同一游戏不能重复提交创建。明确停止运行服务时，先关闭新操作入口，在路径稳定后为停服工作保留存储租约。一边取消排队或准备中的创建和安装任务，一边在各实例锁内停服，无关存储任务不再阻塞所有游戏的保存命令。所有停服尝试结束后才开始备份归档。只有全部存储租约释放后才能取得退出独占权，排空上限为 120 秒。仅关闭主窗口不会请求这一关停。创建在元数据条目或复制、哈希分块之间，以及数据库事务开始前检查取消，取消时清理本次待创建目录。已经开始的文件与数据库事务必须提交或回滚后才释放租约；排空超时列出未结束操作，应用更新中止，最终托盘退出仍保留独立截止。

归档快照同时保留精确程序依赖和个人文件的实际字节。外部独占安装的持久清单区分已保存的修改或未知文件与留在原安装中的原版程序。只删除明确属于实例的原生配置和存档，无法识别归属的文件保留。恢复核验源目录身份和文件摘要，拒绝覆盖内容冲突；缺少可信程序清单时保存完整程序。更新、校验、卸载和可能修改程序的独占启动，在改动归档所需来源前检查依赖。共享归档同样记录精确依赖；完整保存全部程序字节时标为完整归档。完整归档只有验证已保存字节后才能释放源安装，并能重建已消失的原程序目录；存在冲突的替换目录时保留现场并拒绝恢复。已有 v1/v2 归档沿用原恢复路径。归档和删除仍使用不同的持久事务；部分删除必须明确重试，启动恢复不继续破坏性清理。

默认忽略的桌面测试 `native_package_lifecycle` 通过 `LANGAME_NATIVE_MODULE_ID` 和 `LANGAME_NATIVE_PACKAGE_ROOT` 选择一个已有程序包。它将文件独立复制到临时目录，排除声明的原有世界，核验程序权属，执行生产启动入口、模块 `smoke.toml` 就绪探针、停止、配置持久化读回及使用新就绪基线的重启。备份、库卸载、归档恢复与永久删除分别检查对应的保留或移除字节；存在共享引用时必须拒绝库卸载。没有玩家便不产生存档的服务器会如实记录，不宣称已验证存档生成或备份。默认 `LANGAME_NATIVE_ACQUISITION=local_package` 仅通过存储层创建，不认证输入包来源；`official_acquisition` 先在全新的测试目录显式使用生产安装器获取官方程序，再调用桌面创建协调器，并拒绝 `LANGAME_NATIVE_STEAMCMD_ROOT` 覆盖。`LANGAME_NATIVE_INSTANCE_MODE=second_private` 要求官方获取模式，创建两个独立安装：先修改首个独占安装的声明程序入口，加入未知 Mod 和旧存档标记，要求生产创建自动在持久库中补齐原版字节，再将经过验证的程序复制到第二个实例的 runtime 并登记为其所有，同时完整保留首服及其故意修改的字节；第二个安装只使用已验证原版且不继承这些标记。最后删除须保留原库安装和修复后的持久来源，并释放第二个实例的专属程序及登记；删除全部实例副本后仍可复用修复后的库；`shared_pair` 要求模块支持共享，核验共用程序根和独立配置、存档、端口，但另一实例不启动，不代表双服同时运行。所有模式均不替代图形界面或客户端进服验收。

使用非提升的 Windows 进程，测试选择器为 `native_package_lifecycle -- --ignored --nocapture --test-threads=1`。串行批跑时，将单款环境变量替换为 `LANGAME_NATIVE_MODULE_IDS`（逗号分隔，最多 32 项）及 `LANGAME_NATIVE_PACKAGES_ROOT`，按模块声明的安装子目录查找来源；每款单独创建并清理夹具，首个失败立即停止批次。Minecraft 要求操作者事先设置 `LANGAME_SMOKE_MINECRAFT_EULA_ACCEPTED=true`；DST 通过明确的 `LANGAME_NATIVE_DST_OFFLINE_LAN=true` 验证离线局域网模式。仅 SCUM 允许操作者已授权的提升进程，并要求 `LANGAME_NATIVE_ALLOW_ELEVATED_SCUM=true`；夹具在服务器停止后核验并清除自身未被更改的精确防火墙规则，拒绝其他游戏的提升夹具。ProgramData 和用户数据环境变量指向临时目录，但环境变量不能隔离原生 KnownFolder API，仍须使用各游戏的存档路径参数。复制默认限制为 64 GiB、1,200 秒，可用 `LANGAME_NATIVE_COPY_MAX_BYTES` 和 `LANGAME_NATIVE_COPY_TIMEOUT_SECONDS` 设置有界值。测试副本复制与实例创建分别报告耗时。

SteamCMD 可用状态以 `SteamCmdStatus.ready` 为准，不能只看可执行文件是否存在。准备过程必须取得本次运行的 Steam 控制台与 API 初始化成功证据，并确认正常退出、所属进程树已清空。受管理的就绪记录在验证前失效，并绑定当前运行文件；初始化失败或停止不会生成就绪记录。

Steam 服务器安装、更新、校验及创意工坊下载要求 SteamCMD 已通过验证，不再隐式安装或修复此依赖。SteamCMD 未就绪时，游戏库禁用依赖它的安装操作，并通过气泡提示前往系统页执行明确的检查/安装。Minecraft Java 和直链下载安装不受影响。后端在变更程序文件前重复检查依赖，界面在安装操作结束后刷新可用状态。

受管理的 Windows SteamCMD 通过 `app-network/official-sources.json` 中的等价官方源获取当前 win64 签名清单。缺失更新包经大小与 SHA256 校验后原子写入 package 缓存，已有有效缓存直接复用。准备和后续脚本执行期间，临时本机端点向原生 SteamCMD 提供同一份清单，避免再次进入不受控的更新下载。更新应用和最终验证仍由原生 SteamCMD 完成，不伪造 `.installed` 文件或持久化下载主机配置。缺失或损坏的引导程序从清单指定且校验通过的官方 ZIP 中恢复。下载进度累计本次缺失包的实际字节，必要时包含引导程序 ZIP。

忽略执行的 `steamcmd_update_cache::tests::live_direct_install_cache_reuse_and_cancel` 实测需要设置 `NO_PROXY=*`，并将 `LANGAME_STEAMCMD_LIVE_ROOT` 指向尚不存在的绝对诊断目录。测试会下载并运行官方 SteamCMD，验证全新就绪、第二次不重复下载，以及停止后无暂存文件和就绪标记。

SteamCMD 准备与服务器安装、更新、校验通过操作 ID 请求停止。停止确认只设置 `cancel_requested`，执行方必须等子进程、输出读取与待完成文件操作结束后才释放安装锁。只有确认清理完成才进入 `Cancelled`，清理失败仍为 `Failed`。桌面保留活动任务，并在统一底部动态栏提供停止操作。归档/JRE 暂存安装和带保留数据标记的 SteamCMD 重装在停止时恢复原文件；普通 Steam 原生安装、更新保留其部分文件供后续继续处理，无法撤销已更新的文件。

独立的默认忽略选择器 `native_game_tools -- --ignored --nocapture --test-threads=1` 沿用本地程序包隔离前置条件，仅执行生产创建、启动、就绪检查、工具实际效果以及确认停止和清理。先运行 `node apps/desktop/scripts/export_native_gm_cases.cjs <仓库外绝对输出路径.json>` 导出当前前端命令及维护声明动作，再设置 `LANGAME_NATIVE_GM_TOOLS=true` 和指向该文件的 `LANGAME_NATIVE_GM_COMMANDS_FILE`。探针按游戏核对新响应、新日志、存档变化或 DST 原生世界与实体状态。DST 每个分片创建两个原生玩家实体，分别检查库存变化，但不代表客户端进服验收。ARK 世界操作只证明 RCON 响应链路；向已入服玩家发奖励仍须客户端观察。该选择器不替代归档恢复生命周期验收。

需要单独对比原生启动和停止时，使用 `native_start_stop -- --ignored --nocapture --test-threads=1`。它沿用本地程序包隔离、生产就绪和正常停止路径，省略归档与重启阶段。`LANGAME_NATIVE_SETTINGS_JSON` 接受最多 64 KiB 的 JSON 对象，仅覆盖可丢弃实例的 smoke 设置；设置校验与夹具隔离约束仍然生效。`LANGAME_NATIVE_OBSERVE_SECONDS` 支持 0–120 秒观察。后端探针不代表桌面显示、长时间稳定性或客户端进服验收。

The Forest 使用原生 `DllModLoader` 加载内嵌 stdin 桥，仅支持 `help`、`status`、`save`、`shutdown`，不提供玩家管理，也不绕过网络管理员认证。Windows 构建只使用平台 .NET Framework C# 编译器和 BCL，不依赖私有游戏或 Unity DLL。准备阶段核验游戏指纹和独立程序所有权，拒绝覆盖冲突的用户 DLL。保存调用在 Unity 主线程执行，必须确认非空检查点具有新的文件身份；停止先保存、取消并确认自有 stdin 读取线程退出，再调用原生关闭协程，最终仍须完整受管进程树退出才算成功。原生保存目录设置必须带尾分隔符；旧版错位的 `savesMultiplayer` / `savesSinglePlayer` 若非空或不是普通目录，会阻止启动并提示恢复。详见[桥接维护与检查](../modules/theforest/control/README.md)。

ARK 生物扩展由 Windows `build.rs` 调用 `scripts/build_ark_tools.ps1` 在 Cargo `OUT_DIR` 中构建，SDK、导入库和加载器源均固定上游版本及 SHA-256，不向仓库输出二进制。首次构建需要网络和现有 MSVC C++ 工具链；源码与第三方许可见 `modules/ark-tools/`。客户端安装只下载固定摘要的运行框架并发布内嵌插件，不要求用户安装编译器。仅本机桌面 IPC 开放安装和生成；后台持实例锁完成生成与独立 ID 查询，HTTP 管理入口不开放该写操作。

追加 `LANGAME_NATIVE_ARK_CREATURES=true` 会在 ASE/ASA 的隔离实例启动前调用生产安装接口，启动后调用生产生成接口创建 37 级野生渡渡鸟和 150 级野生霸王龙，检查类、等级、实体 ID 读回及相同请求 ID 不重复生成，并验证未知类和离线玩家被拒绝。此选项包含 ASA 匹配符号下载，运行前需取得操作者的明确允许。空服夹具不证明真实在线玩家的驯服归属和客户端显示。

### 多实例与集群事务

目录隔离同时适用于同游戏和不同游戏实例，不等同于沙箱或独立系统账号。模块端口组在创建和启动冲突重映射时作为完整分配单位。ASE 声明 game/peer 偏移 0/1，派生端口被占用时整组移动；确定性探测测试应覆盖偏移组、混合协议组及禁用端口契约，不按类比给 ASA 套用 ASE 契约。

ARK 分组使用版本、集群 ID、规范化目录身份及排序后的成员快照。整组操作保留普通实例生命周期检查并逐项返回结果。本地桌面的集群快照事务锁定 ARK 创建和配置、保留数据库归属事务，并要求所有成员已停止。操作范围明确为每个成员的配置与原生 Saved 树，以及用户确认独占的传输根目录，不猜测原生子目录或恢复其他集群数据。

单个 ASE/ASA 实例通过 `additional_maps` 配置最多 15 张附加地图，共用现有服务端程序、配置、名单和模组。每张启用地图拥有稳定登记端口、独立原生日志、世界目录和 LanGameCMD 目标。暂停保留端口，移除配置保留存档。普通备份覆盖整个原生 Saved 树；历史单图备份仍恢复到主地图目录。运行健康和开服监听检查逐图核对受管进程。可选 `native_package_lifecycle` 验收支持 ASE 的 `LANGAME_NATIVE_ARK_MAPS=ScorchedEarth_P` 或 ASA 的 `ScorchedEarth_WP`，在可丢弃目录核对所有地图端口、新日志、存档目录和整组清理。此能力不改变独立实例之间的程序共享策略，也不证明玩家客户端跨服传输。

完整配置恢复要求快照中的名称、端口、绑定地址和自启动镜像与当前数据库登记一致，预检时拒绝失配，不能依赖以后启动时重新生成配置来修补冲突。公开 API 测试需要经过真实数据库准入，并验证恢复后读回的配置、存档及登记信息。

快照采用有界 manifest、SHA256 校验、普通目录检查和卷实际可用空间检查。当前上限为 128 个 ARK 实例、200,000 个条目、64 层目录、1 TiB 数据、每个集群根目录 1,000 份快照，以及额外保留 512 MiB 容量余量。Windows 容量 API 必须正确处理扩展长路径和 UNC 路径。恢复先发布保护快照，在各目标相邻目录暂存，再以事务记录跟踪发布。实例根的恢复指针使配置目录恰处于重命名间隙时仍可确定恢复身份。显式中断恢复回滚未提交事务或完成已提交事务的清理；启动必须在读取或重新生成配置前拒绝未完成事务。

运行资源策略与本次运行实际采用的策略分别记录。CPU 使用整机百分比，内存使用 Windows Job 内全部实例进程的提交内存总量，包含饥荒两个分片。内存准入计入活动配置预算、待启动预算及主机余量。实例停止后才能修改，下次启动生效；保存配置不能当作现有 Job 已更改的证明。

### 后台运行归属

Windows 当前用户的运行进程拥有服务器 Job、存储事务和恢复任务，界面通过认证的本地 IPC 重连。关闭主窗口只隐藏窗口，保留界面进程和托盘；点击托盘或再次启动会恢复原窗口，WebView 自动重建也保持用户要求的可见状态。界面意外断开不会转移或结束后台运行所有权。托盘退出先将最终停服责任交给独立原生助手，随后退出界面，不等待保存或停服完成。后台共用手动停止的保存、停服流程及实例备份策略，清理完成或达到固定最终截止后自行结束。应用更新先完成下载，再请求停服，仍须收到完成回执且运行服务成功退出后才安装。这是用户会话进程，不是 Windows 系统服务，也不保证在注销或重启后存活。

受管开发启动器在运行服务存活期间保留构建目标租约和前端进程，重新打开时复用运行服务原来的程序和目标。原启动器或前端不可用时应明确报错，不能杀服务、切换目标或启动竞争的运行进程。README 中的可移植源码命令保持不变。

### 在线人数

`runtime.player_count_source` 显式选择 `player_query`（默认 UDP 查询契约）或 `player_list`（已声明的在线玩家来源）。Squad、HumanitZ 使用认证后的 `player_list`，实例概览和系统总人数与玩家中心共享同一份完整列表。只有未过期、完整、未截断且状态为 `ready` 的结果才能提供人数；认证失败、RCON 未启用、进程身份改变或响应不完整时保持未知。自动轮询合并进行中的请求，并按模块的单调时钟刷新周期复用结果，包括失败结果。HumanitZ 保留默认关闭 RCON 的设置，需要操作者配置并启用后才能查询玩家。

没有运行实例查询成功时，系统人数显示未知；仅部分运行实例查询成功时显示已知人数下限。成功取得空列表才显示零人。配置容量仍作为独立数值。选择已验证的玩家列表来源不代表原生 A2S 已通过，也不证明所有服务器模式都不支持 A2S。原生生命周期夹具对 `player_list` 模块在同一次运行中比较生产列表人数、实例概览和全局汇总。

2026-09-21 对 HumanitZ build 23914958 的隔离对照复现了原生 EOS 会话模式下 A2S 无回复；仅在临时副本中把 `OnlineSubsystem.DefaultPlatformService` 改为 `Steam`，同一查询端口便返回合法 A2S_INFO。这证明该版本的查询行为依赖原生会话平台，但不构成修改产品默认平台的依据，因为平台同时控制联机发现。保留原生会话平台，管理器人数使用认证后的玩家列表。

2026-09-21，Squad build 25149748 已载入实际游戏模式、创建默认 RedpointEOS 会话并返回完整认证名单。Windows 套接字事件确认 INFO 请求到达查询端口，但未记录该端口的接收调用或回复；游戏端口和独立 UDP 对照均正常接收。端口绑定本身不能证明原生查询就绪。隔离夹具须保留完整的世界、会话、名单校验，以及安装包中的默认地图投票文件。

Squad 的隔离 Steam 平台对照未能完成原生自动登录和会话创建：Steam 因 `bUsesPresence` 与 `bUseLobbiesIfAvailable` 不一致，在启动服务器广告任务之前拒绝请求。该切换不是兼容修复，不能据此替换默认平台或放宽就绪校验。

### 界面与翻译

修改文字、控件、响应式布局或原生窗口边界时，遵循统一的[桌面字体与窗口布局规范](desktop-ui.md#简体中文)。

活动导航已经表达当前位置时，不要在内容区重复同名标题。只有具备独立对象、状态或操作边界的内容才使用卡片。按需覆盖加载、空状态、错误、禁用、处理中、成功和权限不足状态。

操作确认应留在应用内。破坏性操作通过按钮位置的 `InlineConfirmAction` 展示目标及影响；确认状态绑定目标与相关输入，变更时取消，并返回操作 Promise 以防重复提交。禁止使用浏览器 `window.confirm`、`window.alert`；复杂操作预览使用既有应用内审阅界面。

所有用户可见文案必须同时覆盖英文和简体中文。应修改源词条或生成器，不要手工修改生成后的词条分块。修改后执行：

~~~powershell
npm --prefix apps/desktop run verify:i18n
npm --prefix apps/desktop run verify:i18n-readable
~~~

### 游戏模块与证据

实例的自启动和保存策略统一由维护页管理。`settings/save-policy.ts` 明确列出原生自动存档、轮转备份字段及 Unturned 托管保存间隔的归属；普通配置、模组、玩家页不重复编辑这些字段。保存策略沿用原有 schema 类型与边界、模块交叉字段校验，并只合并修改过的策略字段；保存完成后的确认值与刷新失败状态分别处理。原生参数仍写入游戏原有文件或启动参数，下次启动时应用；停服备份策略保存后生效。

所有游戏始终显示“自动保存”区域：可调策略显示明确的频率输入框或原生开关，没有可调策略时显示实际保存方式和频率限制。潜渊症按战役阶段保存，RimWorld 玩家存档由客户端同步，Squad 不提供可恢复世界存档。Unturned 提供独立开关与秒数输入框；它没有内置自动保存，由 `commands_managed_save.rs` 对已登记且仍存活的受管进程周期发送模块声明的 `Save`：默认 300 秒，0 关闭，策略修改对运行中实例生效。任务随进程运行标识、存储上下文和应用关停清理，待确认的写入禁止重叠；日志中的命令写入成功不等同于游戏已经完成落盘。

保存策略归属与前端生命周期回归包含在 `verify:configuration-workspace` 中；`app-storage` 和 `app-runtime` 的 `save_policy` 测试覆盖 32 模块策略持久化及原生文件、启动参数生成。桌面 `managed_save` 测试使用受控 stdin 进程验证到期、禁用、重启、失去进程所有权和晚到确认，不替代真实游戏存档烟测。

饥荒联机版的世界设置图标优先读取已安装专服的本地图集，并将验证通过的图集保存在 `<app-data-root>/cache/dontstarve-configuration-icons/data/images/`，供专服文件缺失时使用。缓存仅包含 `worldgen_customization.{xml,tex}` 和 `worldsettings_customization.{xml,tex}` 四个文件；有效的安装资源会更新缓存。桌面端通过有并发限制的工作线程解码，再将 PNG 数据交给设置页，不下载图片，也不写入游戏安装目录。两处均无图集时，页面显示提示与重试操作，文字控件仍可正常使用。源码分发和浏览器模拟模式均不附带游戏贴图。

饥荒世界配置的普通 Lua 数据表与引导表单同步。读取时显示实际生效的原生值，编辑时仅修改对应值，保留其他内容和注释。前后端均使用有长度、嵌套深度和条目数量限制的静态解析，不执行用户脚本。包含程序逻辑的完整脚本继续由高级编辑器管理，其对应引导字段只读；完整脚本生效时，额外覆盖行保留但不生效。关闭洞穴只暂停该分片，保留世界和 Mod 设置。

新增游戏，或修改配置、启动、生命周期、端口、查询与玩家管理契约前，请阅读 [游戏接入验证](game-integration-validation.md)。

- 支持结论必须来自权威文档、可合法获取的 Windows 专用服务器程序包或可复现的安装探测。
- 在 `config-sources.toml` 中记录每个已建模原生设置和主动排除项。
- 将确定性、已脱敏的验收夹具保存在 `modules/<module-id>/config-fixtures/`。
- 不得根据游戏客户端、其他游戏、营销文案或未经验证的社区指南推定服务器行为。
- 不得将专有二进制文件、游戏资产、私有日志、本机绝对路径或密钥作为证据提交。

配置来源台账与配置验收记录属于生成产物。修改模块源文件或生成器后执行：

~~~powershell
python -B scripts/verify_game_config_provenance.py --write
python -B scripts/verify_game_config_acceptance.py --require-all --write
~~~

不要手工修改 `docs/game-config-source-ledger.md`、`docs/game-config-source-ledger.csv` 或 `docs/game-config-acceptance/*.md`。

### 依赖

依赖更新应单独提交，除非它与同一修复存在必要关系。更新前应检查上游发布与迁移说明、运行环境与工具链兼容性、许可证变化、供应链影响和锁文件差异。版本更高本身不能证明升级安全。

### 验证

先运行最小相关检查，再按受影响边界扩大范围。可移植的仓库基线为：

~~~powershell
python -B -m unittest discover -s scripts/tests -p "test_*.py"
npm --prefix apps/desktop run verify
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
~~~

游戏配置改动还必须以检查模式运行生成器：

~~~powershell
python -B scripts/verify_game_config_provenance.py --check
python -B scripts/verify_module_setting_coverage.py
python -B scripts/verify_game_config_acceptance.py --require-all --check
python -B scripts/verify_library_media.py
~~~

共享的专用服务器烟测设施依赖受控的存储与生命周期策略，因此不属于公开仓库。上述可选包副本测试在本机使用操作者提供的程序包执行。应分别报告确定性检查与真实运行结果，并说明已脱敏的测试环境。

进程、控制台、日志、调度及 Web 生命周期变更应运行[模拟运行可靠性验收](runtime-reliability.md#简体中文)，记录有限周期资源与故障恢复证据，无需启动真实游戏实例。

### 源码分发

首次公开源码从一个经过审查的根提交开始。替换开发历史前，在仓库外保留并验证恢复备份。本地与公开仓库的 master 使用同一个已审核的源码提交；备份保持私有，不推送其中的旧引用。后续版本正常记录提交，不在每次发布时重置历史。

审查准备发布的精确源码版本，保留代码许可、品牌规则、第三方声明和必要构建输入。准备快照前，运行当前源码检查：

~~~powershell
python -B scripts/verify_no_tracked_secrets.py
python -B scripts/verify_open_source_boundary.py
~~~

当前源码扫描遇到候选文件无法读取或越出仓库边界时会失败，不将未检查内容报告为已通过。不带历史的快照仍须检查其实际包含的文件。

先预览现有导出工具的结果：

~~~powershell
python -B scripts/export_public_snapshot.py --verbose
~~~

预览读取已经跟踪的工作树文件，不包含未跟踪文件，也不等于最终固定版本。审查并提交全部需要分发的源码（包括新增文件）后，在干净工作树中执行导出。不要为了满足干净工作树要求丢弃未完成工作。输出目录必须位于仓库外，且尚不存在：

~~~powershell
python -B scripts/export_public_snapshot.py --execute --output ../langame-public-source
~~~

导出工具复制干净 `HEAD` 的源码，检查文件内容和入口引用，并生成文件哈希清单。Git 历史、本机启动脚本、运行数据和未经许可的资产不会导出。清单保留来源提交 ID 用于追溯，不包含提交对象及其历史。工具不会修改仓库历史，也不会在输出目录初始化 Git 仓库、提交、推送或发布；从较早提交导出的源码不包含之后的未提交改动。

导出时的秘密、产品边界及入口引用检查，不替代完整许可检查、构建或安装包验收。在干净的外部环境审查和构建导出候选后，再单独授权公开。以审核后的快照建立首个公开提交，之后本地与公开远端维护同一套 master 历史。

既有 CI 历史检查继续启用；在公开仓库中，它检查该仓库自身的公开历史，无需导入内部提交。历史检查工具也继续用于内部验证。若未来决定公开任何旧的内部历史，须另行确定范围，除上述当前源码检查外，还需运行：

~~~powershell
python -B scripts/verify_open_source_history.py
~~~

当前文件通过检查，不代表早期提交中的内容已被移除。历史检查仅输出提交、文件、行号与规则，不打印命中的值。发布历史前必须处理这些问题，不能跳过检查，也不能将删除当前文件视为已完成历史清理。检查要求完整克隆，拒绝浅克隆，并读取原始 Git 对象而非本地 `git replace` 替换内容，同时检查提交消息及附注标签消息；消息命中使用 `git-metadata/` 位置标识。Git 作者、提交者的姓名和邮箱应另行核对是否符合预期的公开身份。
