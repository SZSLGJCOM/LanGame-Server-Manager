import type { ModuleDetails } from "../types";
import { mockBootstrap } from "./bootstrap";
import { defaultMockRuntimePerformancePolicy } from "./catalogs";
import { parseMockPlayerListFromModuleToml } from "./live-players";
import { mockModuleSchemasById, parseMockPortRolesFromModuleToml } from "./module-assets";
import { buildMockPorts } from "./module-settings";
import { withMockProcessHostSurface } from "./runtime-helpers";
import {
  parseMockPlayerActionsFromModuleToml,
  parseMockPlayerQueryFromModuleToml,
  parseMockPlayerCountSourceFromModuleToml,
  parseMockPortGroupsFromModuleToml,
  parseMockBindAddressFromModuleToml,
  parseMockRuntimeShutdownFromModuleToml,
  parseMockPlayerManagementFromModuleToml,
  parseMockInstallFromModuleToml,
  parseMockProcessFromModuleToml,
  parseMockWorkshopFromModuleToml,
  parseMockModsFromModuleToml,
  parseMockStorageFromModuleToml,
  parseMockProgramSharingFromModuleToml
} from "./module-manifest";

export function buildMockModuleDetails(moduleId: string): ModuleDetails {
  const summary = mockBootstrap.state.modules.find((item) => item.id === moduleId) ?? mockBootstrap.state.modules[0];
  const tomlProcess = parseMockProcessFromModuleToml(summary.id);
  const isDontStarve = summary.id === "dontstarve";
  const isAbioticFactor = summary.id === "abioticfactor";
  const isArkSurvivalAscended = summary.id === "arksurvivalascended";
  const isArkSurvivalEvolved = summary.id === "arksurvivalevolved";
  const isConanExiles = summary.id === "conanexiles";
  const isCoreKeeper = summary.id === "corekeeper";
  const isEnshrouded = summary.id === "enshrouded";
  const isNecesse = summary.id === "necesse";
  const isPalworld = summary.id === "palworld";
  const isProjectZomboid = summary.id === "projectzomboid";
  const isSevenDaysToDie = summary.id === "sevendaystodie";
  const isTerraria = summary.id === "terraria";
  const isUnturned = summary.id === "unturned";
  const isValheim = summary.id === "valheim";
  const isVRising = summary.id === "vrising";
  const processWindowPolicy = tomlProcess?.window_policy ?? "background";
  const processHostSurface = tomlProcess?.host_surface
    ?? (isArkSurvivalAscended || isArkSurvivalEvolved || isConanExiles
      ? "managed_native_window"
      : "managed_terminal");
  const processHostNotes = tomlProcess?.host_notes ?? (isAbioticFactor
    ? "Abiotic Factor is expected to stay background-hosted. LanGame should own the visible log surface instead of leaving a separate console window on the desktop."
    : isArkSurvivalAscended
      ? "ARK: Survival Ascended should run as a background-hosted dedicated server. LanGame should materialize the live WindowsServer config, admin lists, and cluster storage before direct-launching the server binary."
    : isArkSurvivalEvolved
      ? "ARK: Survival Evolved should stay background-hosted under LanGame. LanGame should materialize the live WindowsServer configs first, then direct-launch the dedicated server binary without leaving a visible local shell window behind."
    : isConanExiles
      ? "Conan Exiles is intended to run as a background-managed dedicated server on Windows. LanGame should materialize the live WindowsServer config root first, then direct-launch the dedicated server binary without leaving a separate shell window open."
    : isCoreKeeper
      ? "Core Keeper should remain background-hosted in LanGame. The host-facing surface belongs in the desktop app, not in a separate console window."
    : isEnshrouded
      ? "Enshrouded should launch as a background-hosted dedicated server. LanGame should materialize the official install-root enshrouded_server.json first, then direct-launch the server so the app remains the operator's primary control surface."
    : isNecesse
      ? "Necesse should stay background-hosted through its bundled Java dedicated server entrypoint. LanGame should direct-launch jre/bin/java.exe with Server.jar and -nogui, supply instance-owned data and log roots first, and keep host-facing console control inside the desktop app."
    : isPalworld
      ? "Palworld should launch the dedicated -Cmd server binary directly. LanGame should materialize the active instance config into the shared WindowsServer config root before launch and own the operator-facing log view."
    : isProjectZomboid
      ? "Project Zomboid should direct-launch its bundled Java runtime on Windows. LanGame should materialize server.ini into the runtime-home server folder, derive the live Java classpath from the install, and keep all host-facing output inside the desktop app."
    : isSevenDaysToDie
      ? "7 Days to Die should launch the dedicated server binary directly in batchmode and nographics mode. The desktop app should absorb the host-facing console experience without leaving a separate shell window open."
    : isTerraria
      ? "TerrariaServer is console-driven, but LanGame should capture that console interaction without leaving a separate terminal window on the Windows desktop."
    : isValheim
      ? "Valheim should launch the dedicated server binary directly in batchmode and nographics. LanGame should materialize the admin, ban, and permit lists into the active save root before launch."
    : isDontStarve
      ? "Don't Starve Together shard processes are expected to stay background-hosted. LanGame should capture their console output and keep shell windows off the desktop."
    : isVRising
      ? "V Rising should stay background-hosted after LanGame materializes the live persistent data layout and Settings files. The desktop app should remain the only operator-facing surface."
    : "This module is expected to stay background-hosted in LanGame.");
  const playerQuery = parseMockPlayerQueryFromModuleToml(summary.id) ?? (summary.id === "dontstarve"
    ? { protocol: "a2s_info", port_names: ["steam_query"] }
    : summary.id === "minecraft"
      ? { protocol: "none", port_names: [] }
    : summary.id === "palworld"
      ? { protocol: "a2s_info", port_names: ["game"] }
      : summary.id === "sevendaystodie"
        ? { protocol: "a2s_info", port_names: ["game_udp"] }
      : ["arksurvivalevolved", "arksurvivalascended", "conanexiles", "enshrouded", "rust", "squad", "valheim", "vrising"].includes(summary.id)
        ? { protocol: "a2s_info", port_names: ["query"] }
      : summary.id === "unturned"
        ? { protocol: "a2s_info", port_names: ["query", "game"] }
      : { protocol: "none", port_names: [] });
  const runtime: ModuleDetails["runtime"] = {
    program_sharing: parseMockProgramSharingFromModuleToml(summary.id),
    bind_address: parseMockBindAddressFromModuleToml(summary.id),
    port_roles: parseMockPortRolesFromModuleToml(summary.id),
    port_groups: parseMockPortGroupsFromModuleToml(summary.id),
    player_count_source: parseMockPlayerCountSourceFromModuleToml(summary.id),
    player_query: playerQuery,
    player_actions: [],
    player_list: parseMockPlayerListFromModuleToml(summary.id),
    player_management: null,
    performance: defaultMockRuntimePerformancePolicy()
  };
  runtime.player_actions = parseMockPlayerActionsFromModuleToml(summary.id);
  runtime.player_management = parseMockPlayerManagementFromModuleToml(summary.id);
  runtime.shutdown = parseMockRuntimeShutdownFromModuleToml(summary.id);
  const manifestWorkshop = parseMockWorkshopFromModuleToml(summary.id);
  const mods = parseMockModsFromModuleToml(summary.id);

  return {
    summary,
    workshop: isDontStarve
      ? {
          provider: "steam",
          consumer_app_id: 322330,
          supports_collections: true
        }
      : isProjectZomboid
        ? {
            provider: "steam",
            consumer_app_id: 108600,
            supports_collections: false
          }
      : isArkSurvivalEvolved
        ? {
            provider: "steam",
            consumer_app_id: 346110,
            supports_collections: false
          }
      : isUnturned
        ? {
            provider: "steam",
            consumer_app_id: 304930,
          supports_collections: false
        }
      : manifestWorkshop,
    mods,
    storage: parseMockStorageFromModuleToml(summary.id),
    schema_json: JSON.stringify(
      mockModuleSchemasById[summary.id] ?? {
        type: "object",
        properties: {
          server_name: { type: "string" },
          max_players: { type: "integer", default: 6 }
        }
      },
      null,
      2
    ),
    default_ports: buildMockPorts(summary.id),
    install: parseMockInstallFromModuleToml(summary.id),
    runtime,
    process: withMockProcessHostSurface(
      isDontStarve
      ? {
          executable: "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe",
          args_template: [
            "-persistent_storage_root",
            "{{paths.config_dir}}",
            "-conf_dir",
            "clusters",
            "-cluster",
            "main",
            "-shard",
            "Master",
            "-bind_ip",
            "{{instance.bind_ip}}",
            "-port",
            "{{ports.master.port}}"
          ],
          window_policy: processWindowPolicy,
          host_notes: processHostNotes
        }
      : isAbioticFactor
        ? {
            executable: "AbioticFactor/Binaries/Win64/AbioticFactorServer-Win64-Shipping.exe",
            args_template: [
              "-PORT={{ports.game.port}}",
              "-QueryPort={{ports.query.port}}",
              "-MaxServerPlayers={{settings.max_server_players}}",
              "-WorldSaveName={{settings.world_save_name}}",
              "{{abioticfactor.sandbox_ini_flag}}",
              "{{abioticfactor.admin_ini_flag}}",
              "-SteamServerName={{settings.server_name}}",
              "{{abioticfactor.server_password_flag}}",
              "{{abioticfactor.admin_password_flag}}",
              "{{abioticfactor.lan_only_flag}}",
              "{{abioticfactor.platform_limited_flag}}",
              "{{abioticfactor.multihome_flag}}",
              "{{abioticfactor.use_local_ips_flag}}",
              "{{abioticfactor.use_perf_threads_flag}}",
              "{{abioticfactor.no_async_loading_thread_flag}}"
            ],
            working_directory_template: "{{paths.install_root}}/AbioticFactor/Binaries/Win64",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isArkSurvivalAscended
        ? {
            executable: "ShooterGame/Binaries/Win64/ArkAscendedServer.exe",
            args_template: [
              "{{arksa.server_url}}",
              "-port={{ports.game.port}}",
              "-WinLiveMaxPlayers={{settings.max_players}}",
              "-NullRHI",
              "-Unattended",
              "-NoSplash",
              "-abslog={{paths.logs_dir}}/ark-ascended-server.log",
              "{{arksa.multihome_flag}}",
              "{{arksa.cluster_dir_override_flag}}",
              "{{arksa.mod_ids_flag}}",
              "{{arksa.official_launch_flags}}",
              "{{arksa.custom_launch_flags}}"
            ],
            working_directory_template: "{{paths.install_root}}/ShooterGame/Binaries/Win64",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isArkSurvivalEvolved
        ? {
            executable: "ShooterGame/Binaries/Win64/ShooterGameServer.exe",
            args_template: [
              "{{arkse.server_url}}",
              "-NullRHI",
              "-Unattended",
              "-NoSplash",
              "-abslog={{paths.logs_dir}}/ark-evolved-server.log",
              "{{arkse.multihome_flag}}",
              "{{arkse.cluster_dir_override_flag}}",
              "{{arkse.official_launch_flags}}",
              "{{arkse.custom_launch_flags}}"
            ],
            working_directory_template: "{{paths.install_root}}/ShooterGame/Binaries/Win64",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isConanExiles
        ? {
            executable: "ConanSandbox/Binaries/Win64/ConanSandboxServer-Win64-Shipping.exe",
            args_template: [
              "ConanSandbox?listen",
              "-Port={{ports.game.port}}",
              "-QueryPort={{ports.query.port}}",
              "-MaxPlayers={{settings.max_players}}",
              "-ServerName={{settings.server_name}}",
              "-RconPort={{ports.rcon.port}}",
              "-server",
              "-log",
              "-useallavailablecores",
              "{{conanexiles.multihome_flag}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isCoreKeeper
        ? {
            executable: "CoreKeeperServer.exe",
            args_template: [
              "-batchmode",
              "-logfile",
              "{{corekeeper.log_path}}",
              "-datapath",
              "{{paths.data_dir}}",
              "{{corekeeper.direct_ip_flag}}",
              "{{corekeeper.direct_ip_value}}",
              "{{corekeeper.direct_port_flag}}",
              "{{corekeeper.direct_port_value}}",
              "{{corekeeper.direct_password_flag}}",
              "{{corekeeper.direct_password_value}}",
              "{{corekeeper.direct_allowed_platform_flag}}",
              "{{corekeeper.direct_allowed_platform_value}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isEnshrouded
        ? {
            executable: "enshrouded_server.exe",
            args_template: [],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isNecesse
        ? {
            executable: "jre/bin/java.exe",
            args_template: [
              "-Dfile.encoding=UTF-8",
              "-jar",
              "Server.jar",
              "-nogui",
              "-world",
              "{{settings.world_name}}",
              "-port",
              "{{ports.game.port}}",
              "-slots",
              "{{settings.max_slots}}",
              "-owner",
              "{{settings.owner_name}}",
              "-motd",
              "{{settings.motd}}",
              "-password",
              "{{settings.password}}",
              "-pausewhenempty",
              "{{necesse.pause_when_empty_value}}",
              "-giveclientspower",
            "{{necesse.strict_server_authority_value}}",
            "-logging",
            "{{necesse.logging_enabled_value}}",
            "-logs",
            "..\\logs",
            "-zipsaves",
            "{{necesse.zip_saves_value}}",
              "-language",
              "{{settings.language}}",
              "-ip",
              "{{instance.bind_ip}}",
              "-datadir",
              "{{paths.data_dir}}",
              "{{necesse.ignore_seasons_flag}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isPalworld
        ? {
            executable: "Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe",
            args_template: [
              "-port={{ports.game.port}}",
              "-players={{settings.max_players}}",
              "-logformat={{settings.log_format}}",
              "{{palworld.public_lobby_flag}}",
              "{{palworld.public_ip_flag}}",
              "{{palworld.public_port_flag}}",
              "{{palworld.use_perf_threads_flag}}",
              "{{palworld.no_async_loading_thread_flag}}",
              "{{palworld.use_multithread_for_ds_flag}}",
              "{{palworld.worker_thread_count_flag}}"
            ],
            working_directory_template: "{{paths.install_root}}/Pal/Binaries/Win64",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isProjectZomboid
        ? {
            executable: "jre64/bin/java.exe",
            args_template: [
              "-Djava.awt.headless=true",
              "-Dzomboid.steam=1",
              "-Dzomboid.znetlog=1",
              "-XX:+UseZGC",
              "-XX:-CreateCoredumpOnCrash",
              "-XX:-OmitStackTraceInFastThrow",
              "-Xms{{settings.memory_gb}}g",
              "-Xmx{{settings.memory_gb}}g",
              "-Duser.home={{paths.config_dir}}/runtime-home",
              "-Djava.library.path=natives/;natives/win64/;.",
              "-cp",
              "{{projectzomboid.classpath}}",
              "zombie.network.GameServer",
              "-statistic",
              "0",
              "-servername",
              "{{instance.id}}",
              "-adminusername",
              "{{settings.admin_username}}",
              "-adminpassword",
              "{{settings.admin_password}}",
              "-port",
              "{{ports.game.port}}",
              "-udpport",
              "{{ports.direct.port}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isValheim
        ? {
            executable: "valheim_server.exe",
            args_template: [
              "-nographics",
              "-batchmode",
              "-name",
              "{{settings.server_name}}",
              "-port",
              "{{ports.game.port}}",
              "-world",
              "{{settings.world_name}}",
              "-password",
              "{{settings.server_password}}",
              "-savedir",
              "{{paths.saves_dir}}",
              "-public",
              "{{settings.public_server}}",
              "-saveinterval",
              "{{settings.save_interval_seconds}}",
              "-backups",
              "{{settings.backup_count}}",
              "-backupshort",
              "{{settings.backup_short_seconds}}",
              "-backuplong",
              "{{settings.backup_long_seconds}}",
              "{{valheim.crossplay_flag}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isSevenDaysToDie
        ? {
            executable: "7DaysToDieServer.exe",
            args_template: [
              "-quit",
              "-batchmode",
              "-nographics",
              "-configfile={{paths.config_dir}}/serverconfig.xml",
              "-dedicated"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isTerraria
        ? {
            executable: "TerrariaServer.exe",
            args_template: [
              "-config",
              "{{paths.config_dir}}/serverconfig.txt",
              "-ip",
              "{{instance.bind_ip}}"
            ],
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : isVRising
        ? {
            executable: "VRisingServer.exe",
            args_template: [
              "-persistentDataPath",
              "{{paths.instance_root}}",
              "{{vrising.bind_address_flag}}",
              "{{vrising.bind_address_value}}"
            ],
            working_directory_template: "{{paths.install_root}}",
            window_policy: processWindowPolicy,
            host_notes: processHostNotes
          }
      : tomlProcess ?? {
          executable: "server.exe",
          args_template: [],
          window_policy: processWindowPolicy,
          host_notes: processHostNotes
        },
      processHostSurface
    )
  };
}
