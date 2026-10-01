import type { InstanceSummary, PortBinding } from "../types";
import {
  mockModuleSchemasById,
  parseMockDefaultPortsFromModuleToml,
  type MockSchemaObject
} from "./module-assets";
function buildMockDontStarveSettings(name: string) {
  return {
    cluster_name: name,
    cluster_description: "Preview DST room for the guided room, world, and mod settings flow.",
    max_players: 6,
    game_mode: "survival",
    cluster_password: "",
    pause_when_empty: false,
    pvp: false,
    vote_enabled: true,
    cluster_intention: "default",
    offline_cluster: true,
    lan_only_cluster: true,
    tick_rate: 15,
    autosaver_enabled: true,
    enable_caves: true,
    world_specialevent: "default",
    world_autumn: "default",
    world_winter: "default",
    world_spring: "default",
    world_summer: "default",
    world_extrastartingitems: "default",
    world_seasonalstartingitems: "default",
    world_spawnprotection: "default",
    world_dropeverythingondespawn: "default",
    world_darkness: "default",
    world_temperaturedamage: "default",
    world_hunger: "default",
    world_healthpenalty: "always",
    world_shadowcreatures: "default",
    world_brightmarecreatures: "default",
    master_world_size: "default",
    master_season_start: "default",
    master_task_set: "default",
    master_start_location: "default",
    master_day: "default",
    master_branching: "default",
    master_loop: "default",
    master_touchstone: "default",
    master_roads: "default",
    master_boons: "default",
    master_prefabswaps_start: "default",
    master_petrification: "default",
    master_meteorshowers: "default",
    master_regrowth: "default",
    master_weather: "default",
    master_frogs: "default",
    master_hounds: "default",
    master_lightning: "default",
    master_wildfires: "default",
    master_berrybush: "default",
    master_carrot: "default",
    master_flint: "default",
    master_grass: "default",
    master_marshbush: "default",
    master_reeds: "default",
    master_rock: "default",
    master_sapling: "default",
    master_trees: "default",
    master_bees: "default",
    master_beefalo: "default",
    master_butterfly: "default",
    master_buzzard: "default",
    master_catcoon: "default",
    master_moles: "default",
    master_pigs: "default",
    master_rabbits: "default",
    master_lightninggoat: "default",
    master_spiders: "default",
    master_tallbirds: "default",
    master_tentacles: "default",
    master_krampus: "default",
    master_walrus: "default",
    master_merm: "default",
    master_houndmound: "default",
    master_lureplants: "default",
    master_bearger: "default",
    master_beequeen: "default",
    master_deerclops: "default",
    master_dragonfly: "default",
    master_goosemoose: "default",
    master_spiderqueen: "default",
    master_liefs: "default",
    master_toadstool: "default",
    caves_world_size: "default",
    caves_branching: "default",
    caves_loop: "default",
    caves_atriumgate: "default",
    caves_wormattacks: "default",
    caves_earthquakes: "default",
    caves_regrowth: "default",
    caves_banana: "default",
    caves_cave_ponds: "default",
    caves_fern: "default",
    caves_flint: "default",
    caves_lichen: "default",
    caves_marshbush: "default",
    caves_mushroom: "default",
    caves_rock: "default",
    caves_sapling: "default",
    caves_bunnymen: "default",
    caves_rocky: "default",
    caves_slurper: "default",
    caves_slurtles: "default",
    caves_monkey: "default",
    caves_bats: "default",
    caves_worms: "default",
    caves_spiders: "default",
    caves_cave_spiders: "default",
    caves_tentacles: "default",
    caves_molebats: "default",
    caves_nightmarecreatures: "default",
    caves_spider_dropper: "default",
    master_world_overrides_extra: "",
    master_worldgenoverride_lua: "return {\n  override_enabled = true,\n  settings_preset = \"SURVIVAL_TOGETHER\",\n  worldgen_preset = \"SURVIVAL_TOGETHER\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n",
    caves_world_overrides_extra: "",
    caves_worldgenoverride_lua: "return {\n  override_enabled = true,\n  settings_preset = \"DST_CAVE\",\n  worldgen_preset = \"DST_CAVE\",\n  overrides = {\n    world_size = \"default\",\n  }\n}\n",
    cluster_token: "",
    admin_list: "",
    whitelist: "",
    whitelist_slots: 0,
    steam_group_only: false,
    steam_group_id: 0,
    steam_group_admins: false,
    shared_workshop_mod_ids: "2039181790\n1909182187",
    shared_workshop_collection_ids: "3495871201",
    master_enabled_workshop_mod_ids: "2039181790",
    caves_enabled_workshop_mod_ids: "1909182187",
    master_mod_configuration_options: {
      "2039181790": {
        language: "zh",
        range_ring: true,
        marker_scale: 1.25
      }
    },
    caves_mod_configuration_options: {
      "1909182187": {
        language: "zh",
        show_creature_age: false
      }
    },
    master_modoverrides_lua: "return {\n}\n",
    caves_modoverrides_lua: "return {\n}\n"
  };
}

const mockAbioticFactorSchema = mockModuleSchemasById.abioticfactor;
const mockCoreKeeperSchema = mockModuleSchemasById.corekeeper;
const mockNecesseSchema = mockModuleSchemasById.necesse;
const mockPalworldSchema = mockModuleSchemasById.palworld;
const mockProjectZomboidSchema = mockModuleSchemasById.projectzomboid;
const mockSevenDaysToDieSchema = mockModuleSchemasById.sevendaystodie;

function buildMockSchemaDefaults(
  schema: MockSchemaObject,
  identity?: { instanceId?: string; instanceName?: string }
) {
  const defaults: Record<string, unknown> = {};

  for (const [key, property] of Object.entries(schema.properties ?? {})) {
    if (!property) {
      continue;
    }

    if (property["x-lsgm-default-source"] === "instance_id" && identity?.instanceId) {
      defaults[key] = identity.instanceId;
      continue;
    }

    if (property["x-lsgm-default-source"] === "instance_name" && identity?.instanceName) {
      defaults[key] = identity.instanceName;
      continue;
    }

    if (property["x-lsgm-default-source"] === "generated_secret") {
      if (!globalThis.crypto?.getRandomValues) {
        throw new Error("Secure random generation is unavailable");
      }
      const bytes = globalThis.crypto.getRandomValues(new Uint8Array(24));
      const secret = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
      const generatedLength = property["x-lsgm-generated-secret-length"];
      const defaultLength = typeof generatedLength === "number" && Number.isSafeInteger(generatedLength) && generatedLength > 0
        ? Math.min(secret.length, generatedLength)
        : secret.length;
      const maxLength = property.maxLength;
      const length = typeof maxLength === "number" && Number.isSafeInteger(maxLength) && maxLength >= 0
        ? Math.min(defaultLength, maxLength)
        : defaultLength;
      defaults[key] = secret.slice(0, length);
      continue;
    }

    if (Object.prototype.hasOwnProperty.call(property, "default")) {
      defaults[key] = property.default;
    }
  }

  return defaults;
}

function buildMockAbioticFactorSettings(name: string, instanceId: string) {
  return {
    ...buildMockSchemaDefaults(mockAbioticFactorSchema, {
      instanceId,
      instanceName: name
    })
  };
}

function buildMockCoreKeeperSettings(name: string, instanceId: string) {
  return {
    ...buildMockSchemaDefaults(mockCoreKeeperSchema, {
      instanceId,
      instanceName: name
    }),
    server_name: name,
    game_id: "",
    max_players: 8,
    world_index: 2,
    world_seed: "AncientAzeos",
    world_mode: 0,
    season_override: -1,
    max_packets_per_frame: 2,
    network_send_rate: 30,
    direct_connection_enabled: true,
    allowed_platform_code: 0,
    admin_list: "76561198077777777\n76561198000000000",
    ban_list: ""
  };
}

function buildMockNecesseSettings(name: string, instanceId: string) {
  return {
    ...buildMockSchemaDefaults(mockNecesseSchema, {
      instanceId,
      instanceName: name
    }),
    world_name: name,
    max_slots: 10,
    motd: "Managed Windows host. Backup before risky restores or patch-day testing.",
    owner_name: "HostLead",
    pause_when_empty: true,
    strict_server_authority: true,
    logging_enabled: true,
    zip_saves: true,
    language: "en",
    ignore_seasons: false
  };
}

function buildMockPalworldSettings(name: string) {
  return {
    ...buildMockSchemaDefaults(mockPalworldSchema, {
      instanceName: name
    })
  };
}

function buildMockProjectZomboidSettings(name: string, instanceId: string) {
  return {
    ...buildMockSchemaDefaults(mockProjectZomboidSchema, {
      instanceId,
      instanceName: name
    }),
    welcome_message: "Welcome to our Friday co-op world.\nRead the house rules before looting LV.",
    server_description: "Long-running co-op host with curated Workshop content.",
    max_players: 8,
    public_server: false,
    open_server: true,
    auto_create_user_in_whitelist: true,
    map_name: "Muldraugh, KY\nRavenCreek",
    workshop_items: "2945221351\n3000065999",
    mods: "RavenCreek\nSkillRecoveryJournal",
    admin_username: "admin",
    memory_gb: 6
  };
}

function buildMockSevenDaysToDieSettings(name: string, instanceId: string) {
  return {
    ...buildMockSchemaDefaults(mockSevenDaysToDieSchema, {
      instanceId,
      instanceName: name
    }),
    server_description: "Windows-first co-op host with explicit horde-night pacing and native access lists.",
    server_login_confirmation_text: "Back up the world before risky restores. Keep blood-moon nights coordinated.",
    server_password: "horde-night",
    max_players: 8,
    visibility: 1,
    region: "Asia",
    language: "Chinese",
    server_disabled_network_protocols: "SteamNetworking",
    server_max_world_transfer_speed_kibs: 512,
    game_world: "RWG",
    game_mode: "GameModeSurvival",
    world_seed: "LanGameHorde",
    world_size: 6144,
    player_killing_mode: 0,
    max_spawned_zombies: 60,
    max_spawned_animals: 40,
    max_uncovered_map_chunks_per_player: 131072,
    land_claim_count: 2,
    land_claim_size: 41,
    land_claim_offline_durability_modifier: 4,
    dynamic_mesh_enabled: true,
    persistent_player_profiles: true,
    web_dashboard_enabled: true,
    web_dashboard_url: "https://hostpanel.example.com:8080/",
    enable_map_rendering: true,
    telnet_enabled: true,
    hide_command_execution_log: 1,
    twitch_server_permission: 90,
    twitch_blood_moon_allowed: false,
    admin_users: [
      {
        platform: "EOS",
        userid: "00000000000000000000000000000001",
        name: "Host Lead",
        permission_level: 0
      }
    ],
    admin_groups: [
      {
        steam_id: "103582791434672565",
        name: "LanGame Hosts",
        permission_level_default: 1000,
        permission_level_mod: 0
      }
    ],
    whitelist_users: [
      {
        platform: "EOS",
        userid: "00000000000000000000000000000001",
        name: "Host Lead"
      },
      {
        platform: "EOS",
        userid: "00000000000000000000000000000002",
        name: "Builder Squad"
      }
    ],
    whitelist_groups: [
      {
        steam_id: "103582791434672566",
        name: "Weekend Survivors"
      }
    ],
    blacklist_entries: [
      {
        platform: "Steam",
        userid: "76561198000000999",
        name: "Known Raider",
        unbandate: "9999-12-31",
        reason: "Griefing"
      }
    ]
  };
}

function buildMockGenericSchemaSettings(moduleId: string, name: string, instanceId: string): Record<string, unknown> {
  const schema = mockModuleSchemasById[moduleId] ?? { properties: {} };
  const defaults = buildMockSchemaDefaults(schema, {
    instanceId,
    instanceName: name
  });

  if (Object.prototype.hasOwnProperty.call(schema.properties ?? {}, "server_name")) {
    defaults.server_name = name;
  }

  return defaults;
}

function buildMockTerrariaSettings(name: string) {
  return {
    world_name: name,
    world_file: "meadow-journey.wld",
    motd: "LanGame Terraria host is live. Bring your picks and wire kit.",
    max_players: 8,
    world_size: 2,
    difficulty: 3,
    seed: "LanGameJourney",
    special_seed: "none",
    worldrollbackstokeep: 3,
    password: "greenleaf",
    secure: true,
    steam: true,
    lobby: "friends",
    banlist_entries: "BadActor42\n203.0.113.24",
    language: "zh-Hans",
    upnp: false,
    npcstream: 60,
    slowliquids: true,
    disableannouncementbox: false,
    announcementboxrange: -1,
    priority: 1,
    journeypermission_time_setfrozen: 1,
    journeypermission_time_setdawn: 1,
    journeypermission_time_setnoon: 1,
    journeypermission_time_setdusk: 1,
    journeypermission_time_setmidnight: 1,
    journeypermission_time_setspeed: 1,
    journeypermission_godmode: 1,
    journeypermission_setdifficulty: 2,
    journeypermission_setspawnrate: 1,
    journeypermission_increaseplacementrange: 2,
    journeypermission_biomespread_setfrozen: 1,
    journeypermission_wind_setstrength: 2,
    journeypermission_wind_setfrozen: 2,
    journeypermission_rain_setstrength: 2,
    journeypermission_rain_setfrozen: 2
  };
}

export function buildMockBackupUsesDeclaredSavesPath(summary: Pick<InstanceSummary, "module_id">): boolean {
  return !["conanexiles"].includes(summary.module_id);
}

export function buildMockPorts(moduleId: string): PortBinding[] {
  const manifestPorts = parseMockDefaultPortsFromModuleToml(moduleId);
  if (manifestPorts.length > 0) {
    return manifestPorts;
  }

  if (moduleId === "dontstarve") {
    return [
      { name: "master", protocol: "udp", port: 10999 },
      { name: "caves", protocol: "udp", port: 11000 },
      { name: "steam_query", protocol: "udp", port: 27016 },
      { name: "steam_auth", protocol: "udp", port: 8766 }
    ];
  }

  if (moduleId === "palworld") {
    return [
      { name: "game", protocol: "udp", port: 8211 },
      { name: "rcon", protocol: "tcp", port: 25575 },
      { name: "rest_api", protocol: "tcp", port: 8212 }
    ];
  }

  if (moduleId === "minecraft") {
    return [
      { name: "game", protocol: "tcp", port: 25565 },
      { name: "rcon", protocol: "tcp", port: 25575 }
    ];
  }

  if (moduleId === "abioticfactor") {
    return [
      { name: "game", protocol: "udp", port: 7777 },
      { name: "query", protocol: "udp", port: 27015 }
    ];
  }

  if (moduleId === "barotrauma") {
    return [
      { name: "game", protocol: "udp", port: 27015 },
      { name: "query", protocol: "udp", port: 27016 }
    ];
  }

  if (moduleId === "arksurvivalascended" || moduleId === "arksurvivalevolved") {
    return [
      { name: "game", protocol: "udp", port: 7777 },
      { name: "peer", protocol: "udp", port: 7778 },
      { name: "query", protocol: "udp", port: 27015 },
      { name: "rcon", protocol: "tcp", port: 27020 }
    ];
  }

  if (moduleId === "conanexiles") {
    return [
      { name: "game", protocol: "udp", port: 7777 },
      { name: "query", protocol: "udp", port: 27015 },
      { name: "rcon", protocol: "tcp", port: 25575 }
    ];
  }

  if (moduleId === "corekeeper") {
    return [{ name: "game", protocol: "udp", port: 27015 }];
  }

  if (moduleId === "enshrouded") {
    return [{ name: "query", protocol: "udp", port: 15637 }];
  }


  if (moduleId === "necesse") {
    return [{ name: "game", protocol: "udp", port: 14159 }];
  }

  if (moduleId === "projectzomboid") {
    return [
      { name: "game", protocol: "udp", port: 16261 },
      { name: "direct", protocol: "udp", port: 16262 },
      { name: "rcon", protocol: "tcp", port: 27015 }
    ];
  }

  if (moduleId === "rust") {
    return [
      { name: "game", protocol: "udp", port: 28015 },
      { name: "rcon", protocol: "tcp", port: 28016 },
      { name: "query", protocol: "udp", port: 28017 }
    ];
  }

  if (moduleId === "terraria") {
    return [{ name: "game", protocol: "tcp", port: 7777 }];
  }

  if (moduleId === "sevendaystodie") {
    return [
      { name: "game_udp", protocol: "udp", port: 26900 },
      { name: "game_tcp", protocol: "tcp", port: 26900 },
      { name: "web_dashboard", protocol: "tcp", port: 8080 },
      { name: "telnet", protocol: "tcp", port: 8081 }
    ];
  }

  if (moduleId === "squad") {
    return [
      { name: "game", protocol: "udp", port: 7787 },
      { name: "query", protocol: "udp", port: 27165 },
      { name: "rcon", protocol: "tcp", port: 21114 }
    ];
  }

  if (moduleId === "unturned") {
    return [
      { name: "game", protocol: "udp", port: 27015 },
      { name: "query", protocol: "udp", port: 27016 }
    ];
  }

  if (moduleId === "valheim") {
    return [
      { name: "game", protocol: "udp", port: 2456 },
      { name: "query", protocol: "udp", port: 2457 }
    ];
  }

  if (moduleId === "vrising") {
    return [
      { name: "game", protocol: "udp", port: 9876 },
      { name: "query", protocol: "udp", port: 9877 },
      { name: "rcon", protocol: "tcp", port: 25575 }
    ];
  }

  return [
    { name: "game", protocol: "udp", port: 10999 },
    { name: "steam", protocol: "udp", port: 27016 }
  ];
}

function buildMockModuleSettings(moduleId: string, name: string, instanceId: string): Record<string, unknown> {
  if (moduleId === "dontstarve") {
    return buildMockDontStarveSettings(name);
  }

  if (moduleId === "palworld") {
    return buildMockPalworldSettings(name);
  }

  if (moduleId === "minecraft") {
    return {
      ...buildMockGenericSchemaSettings(moduleId, name, instanceId),
      motd: name,
      enable_rcon: false
    };
  }

  if (moduleId === "abioticfactor") {
    return buildMockAbioticFactorSettings(name, instanceId);
  }

  if (moduleId === "barotrauma") {
    return {
      server_name: name,
      server_message: "Managed by LanGame Server Manager.",
      server_password: "",
      admin_entries: "",
      max_players: 16,
      public_server: true,
      voice_chat_enabled: true,
      allow_file_transfers: true,
      karma_enabled: true
    };
  }

  if (moduleId === "arksurvivalascended") {
    return {
      server_name: name,
      map_name: "TheIsland_WP",
      max_players: 30,
      battleye_enabled: false,
      server_game_log: true
    };
  }

  if (moduleId === "arksurvivalevolved") {
    return {
      server_name: name,
      map_name: "TheIsland",
      battleye_enabled: false,
      auto_managed_mods: true,
      active_mod_ids: ""
    };
  }

  if (moduleId === "conanexiles") {
    return {
      server_name: name,
      excluded_regions: "",
      max_players: 20,
      rcon_enabled: true
    };
  }

  if (moduleId === "corekeeper") {
    return buildMockCoreKeeperSettings(name, instanceId);
  }

  if (moduleId === "enshrouded") {
    return {
      server_name: name,
      max_players: 16
    };
  }


  if (moduleId === "necesse") {
    return buildMockNecesseSettings(name, instanceId);
  }

  if (moduleId === "projectzomboid") {
    return buildMockProjectZomboidSettings(name, instanceId);
  }

  if (moduleId === "rust") {
    return {
      server_name: name,
      max_players: 100,
      rcon_web: true,
      owner_entries: "",
      moderator_entries: "",
      skip_queue_entries: "",
      banned_entries: ""
    };
  }

  if (moduleId === "terraria") {
    return buildMockTerrariaSettings(name);
  }

  if (moduleId === "sevendaystodie") {
    return buildMockSevenDaysToDieSettings(name, instanceId);
  }

  if (moduleId === "squad") {
    return {
      server_name: name,
      max_players: 80,
      reserved_slots: 0,
      admin_steam_ids: "",
      priority_join_steam_ids: "",
      admin_permissions: "changemap\ncheat\nprivate\nbalance\nchat\nkick\nban\nconfig\ncameraman\ndebug\npause\nimmunity\nmanageserver\nfeaturetest\nreserve\nteamchange\nforceteamchange\ncanseeadminchat",
      admins_cfg: ""
    };
  }

  if (moduleId === "unturned") {
    return {
      server_name: name,
      internet_server: true,
      map: "PEI",
      game_mode: "Normal",
      difficulty: "Normal",
      perspective: "Both",
      max_players: 24,
      password: "",
      owner_steam_id: "",
      admin_steam_ids: "",
      whitelist_enabled: false,
      battl_eye: true,
      hide_admins: true
    };
  }

  return buildMockGenericSchemaSettings(moduleId, name, instanceId);
}

export function buildMockSettingsForModule(moduleId: string, name: string, instanceId: string): Record<string, unknown> {
  const settings = buildMockModuleSettings(moduleId, name, instanceId);
  const schema = mockModuleSchemasById[moduleId];
  const properties = Object.fromEntries(Object.entries(schema?.properties ?? {}).filter(([key, property]) =>
    property?.["x-lsgm-default-source"] === "generated_secret" && !Object.prototype.hasOwnProperty.call(settings, key)
  ));
  return { ...buildMockSchemaDefaults({ properties }), ...settings };
}

export function buildMockSavesPath(
  summary: Pick<InstanceSummary, "id" | "module_id">,
  settings?: Record<string, unknown> | null
): string {
  if (summary.module_id === "dontstarve") {
    return `D:/LanGame/instances/${summary.id}/config/clusters/main`;
  }

  if (summary.module_id === "palworld") {
    return `D:/LanGame/instances/${summary.id}/runtime/Pal/Saved/SaveGames/0/${summary.id}`;
  }

  if (summary.module_id === "arksurvivalevolved") {
    return `D:/LanGame/instances/${summary.id}/runtime/ShooterGame/Saved/${summary.id}`;
  }

  if (summary.module_id === "arksurvivalascended") {
    return `D:/LanGame/instances/${summary.id}/runtime/ShooterGame/Saved/${summary.id}`;
  }

  if (summary.module_id === "abioticfactor") {
    const worldSaveName =
      typeof settings?.world_save_name === "string" && settings.world_save_name.trim().length > 0
        ? settings.world_save_name.trim()
        : summary.id;
    return `D:/LanGame/instances/${summary.id}/runtime/AbioticFactor/Saved/SaveGames/Server/Worlds/${worldSaveName}`;
  }

  if (summary.module_id === "projectzomboid") {
    return `D:/LanGame/instances/${summary.id}/config/runtime-home/Zomboid/Saves/Multiplayer/${summary.id}`;
  }

  if (summary.module_id === "corekeeper") {
    return `D:/LanGame/instances/${summary.id}/data/worlds`;
  }

  if (summary.module_id === "enshrouded") {
    return `D:/LanGame/instances/${summary.id}/savegame`;
  }

  if (summary.module_id === "necesse") {
    return `D:/LanGame/instances/${summary.id}/data/saves`;
  }

  if (summary.module_id === "vrising") {
    return `D:/LanGame/instances/${summary.id}/Saves`;
  }

  return `D:/LanGame/instances/${summary.id}/saves`;
}
