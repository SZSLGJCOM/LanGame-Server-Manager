import type {
  AssistantRunInput,
  AssistantSecretDescriptor,
  DstModConfigurationSpec,
  InstanceDetails,
  ProjectZomboidWorkshopModsSnapshot,
  RuntimePerformancePolicy,
  RuntimePerformancePolicyPreview,
  SteamNewsItem,
  SteamReviewSummary,
  SteamWorkshopLookupItem,
  SteamWorkshopBrowseKind,
  SteamWorkshopSearchResult
} from "../types";
import { readResourceLimits } from "../runtime-resource-policy";

function clone<T>(value: T): T {
  return typeof structuredClone === "function"
    ? structuredClone(value)
    : JSON.parse(JSON.stringify(value)) as T;
}

export const mockSteamNewsCatalog: Record<number, SteamNewsItem[]> = {
  251570: [
    {
      gid: "mock-251570-1",
      title: "V2.6 Stable is live",
      url: "https://store.steampowered.com/news/app/251570/view/mock-1",
      author: "The Fun Pimps",
      feed_label: "Steam News",
      excerpt: "City hitching, dew collector balance, zombie XP tuning, and several world generation fixes landed in this stable update.",
      published_at_unix_ms: Date.now() - 1000 * 60 * 60 * 24 * 2
    },
    {
      gid: "mock-251570-2",
      title: "Dedicated server hosting FAQ refreshed",
      url: "https://store.steampowered.com/news/app/251570/view/mock-2",
      author: "The Fun Pimps",
      feed_label: "Steam News",
      excerpt: "The latest hosting notes focus on ports, mod compatibility, EAC choices, and safer first-run world sizes for new servers.",
      published_at_unix_ms: Date.now() - 1000 * 60 * 60 * 24 * 7
    },
    {
      gid: "mock-251570-3",
      title: "Console crossplay rollout update",
      url: "https://store.steampowered.com/news/app/251570/view/mock-3",
      author: "The Fun Pimps",
      feed_label: "Steam News",
      excerpt: "Crossplay rollout work is still progressing, with version parity and visibility rules remaining key items for multiplayer hosts.",
      published_at_unix_ms: Date.now() - 1000 * 60 * 60 * 24 * 13
    }
  ]
};
export const mockSteamReviewSummaryCatalog: Record<number, SteamReviewSummary> = {
  1623730: {
    app_id: 1623730,
    review_score: 8,
    review_score_desc: "Very Positive",
    total_positive: 8700,
    total_negative: 1300,
    total_reviews: 10000,
    positive_percent: 87,
    source_url: "https://store.steampowered.com/app/1623730/#app_reviews_hash"
  },
  251570: {
    app_id: 251570,
    review_score: 8,
    review_score_desc: "Very Positive",
    total_positive: 92000,
    total_negative: 15000,
    total_reviews: 107000,
    positive_percent: 86,
    source_url: "https://store.steampowered.com/app/251570/#app_reviews_hash"
  },
  361420: {
    app_id: 361420,
    review_score: 8,
    review_score_desc: "Very Positive",
    total_positive: 126799,
    total_negative: 11105,
    total_reviews: 137904,
    positive_percent: 92,
    source_url: "https://store.steampowered.com/app/361420/#app_reviews_hash"
  }
};
export function localizeMockSteamReviewSummary(summary: SteamReviewSummary | undefined, locale: unknown): SteamReviewSummary | null {
  if (!summary) {
    return null;
  }

  const localized = clone(summary);
  if (String(locale ?? "").toLowerCase().startsWith("zh")) {
    const zhScoreDescriptions: Record<string, string> = {
      "Very Positive": "特别好评"
    };
    localized.review_score_desc = zhScoreDescriptions[localized.review_score_desc] ?? localized.review_score_desc;
  }
  return localized;
}
export const sampleSteamAboutHtml = "<h2>Reviews</h2><div class=\"steam-review-copy\">&ldquo;This game is the best Zombie FPS that I have ever played, it has a great mix of building and survival.&rdquo;<br>Worth a Buy Guys<br><br>&ldquo;7 Days to Die is like Minecraft with Gravity. Building defenses with friends and huddling in the corner hoping no zombies get through can be great fun. But what keeps me coming back to the game is the TNT.&rdquo;<br>Kotaku<br><br>&ldquo;7 Days to Die looks genuinely impressive, and I can't see this not making a huge impact when it arrives.&rdquo;<br>Rock, Paper, Shotgun</div><hr /><p class=\"bb_paragraph\"><span class=\"bb_img_ctn\"><img class=\"bb_img\" src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/251570/extras/6a28b81698094dc5aecf89a79ec9088b.avif?t=1774294721\" width=\"610\" height=\"148\" /></span></p><h2>About This Game</h2><p class=\"bb_paragraph\">7 Days to Die is an open-world game that blends first-person shooter, survival horror, tower defense, and role-playing systems.</p><h2>Game Features</h2><ul class=\"bb_ul\"><li><p class=\"bb_paragraph\"><strong>Explore</strong> - Huge environments with multiple biomes and flexible play styles.</p></li><li><p class=\"bb_paragraph\"><strong>Craft</strong> - Weapons, armor, tools, vehicles, and more than 500 recipes.</p></li><li><p class=\"bb_paragraph\"><strong>Cooperate or Compete</strong> - Supports PvE, PvP, and multiplayer hosting.</p></li></ul>";

export const mockWorkshopLookupCatalog: Record<string, SteamWorkshopLookupItem> = {
  "2039181790": {
    id: "2039181790",
    title: "Global Positions",
    preview_url: "https://images.steamusercontent.com/ugc/535134943326895647/0FF32BFCBDF3CDAFD88373F2D76C9DDE50D8C090/",
    description_excerpt: "Shared minimap position markers for DST players.",
    description: "[h1]Global Positions[/h1]\nShare player and map positions across the server. Includes configurable indicators, colors, and map visibility rules.",
    creator_id: "synthetic-creator-0001",
    file_size: 418732,
    created_at_unix: 1586681520,
    updated_at_unix: 1778975400,
    subscriptions: 1482000,
    favorites: 82340,
    views: 2136000,
    tags: ["Server", "Map", "Utility"],
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=2039181790",
    item_kind: "item",
    status: "resolved",
    message: null,
    consumer_app_id: 322330,
    creator_app_id: 322330,
    child_count: 0,
    children: []
  },
  "1909182187": {
    id: "1909182187",
    title: "Insight",
    preview_url: "https://images.steamusercontent.com/ugc/1479948484468384018/FD83481D00C2D0CDB02F6D53D6A957A976CE95ED/",
    description_excerpt: "Adds rich inspect overlays and entity data for DST servers.",
    description: "[h1]Insight[/h1]\nInspect creatures, resources, food, weather, and world objects with a configurable information overlay.\n\n[quote]Designed for shared worlds and dedicated servers.[/quote]",
    creator_id: "synthetic-creator-0002",
    file_size: 3841572,
    created_at_unix: 1573069980,
    updated_at_unix: 1780021800,
    subscriptions: 964000,
    favorites: 51420,
    views: 1728000,
    tags: ["Server", "Interface", "Quality of Life"],
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=1909182187",
    item_kind: "item",
    status: "resolved",
    message: null,
    consumer_app_id: 322330,
    creator_app_id: 322330,
    child_count: 0,
    children: []
  },
  "3495871201": {
    id: "3495871201",
    title: "Friday Night DST Collection",
    preview_url: "https://images.steamusercontent.com/ugc/925933343645560363/6A04CE61D30E0BADE22BEFA3D71DFFAEDF07F74A/",
    description_excerpt: "A curated pack of shared quality-of-life server mods for DST.",
    description: "A small, compatible set of quality-of-life mods for recurring dedicated-server sessions. The collection keeps map sharing and inspect information together.",
    creator_id: "synthetic-creator-0003",
    created_at_unix: 1739707200,
    updated_at_unix: 1777766400,
    subscriptions: 18400,
    favorites: 2100,
    views: 35600,
    tags: ["Collection", "Server", "Quality of Life"],
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=3495871201",
    item_kind: "collection",
    status: "resolved",
    message: null,
    consumer_app_id: 322330,
    creator_app_id: 322330,
    child_count: 2,
    children: [
      {
        id: "2039181790",
        item_kind: "item",
        status: "resolved",
        title: "Global Positions",
        preview_url: "https://images.steamusercontent.com/ugc/535134943326895647/0FF32BFCBDF3CDAFD88373F2D76C9DDE50D8C090/",
        consumer_app_id: 322330
      },
      {
        id: "1909182187",
        item_kind: "item",
        status: "resolved",
        title: "Insight",
        preview_url: "https://images.steamusercontent.com/ugc/1479948484468384018/FD83481D00C2D0CDB02F6D53D6A957A976CE95ED/",
        consumer_app_id: 322330
      }
    ]
  },
  "2945221351": {
    id: "2945221351",
    title: "Raven Creek",
    preview_url: "https://images.steamusercontent.com/ugc/1479949115738031219/4E57615BD1A111E1CB4A02C591B396A88DE51324/",
    description_excerpt: "Large urban map expansion for Project Zomboid.",
    description: "[h1]Raven Creek[/h1]\nA dense urban region with custom buildings, loot routes, and multiple approaches into the city. Add its map IDs to the server map order after download.",
    creator_id: "synthetic-creator-0004",
    file_size: 786432000,
    created_at_unix: 1690732800,
    updated_at_unix: 1774224000,
    subscriptions: 728000,
    favorites: 48200,
    views: 1240000,
    tags: ["Map", "Multiplayer", "Build 42"],
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=2945221351",
    item_kind: "item",
    status: "resolved",
    message: null,
    consumer_app_id: 108600,
    creator_app_id: 108600,
    child_count: 0,
    children: []
  },
  "3000065999": {
    id: "3000065999",
    title: "Skill Recovery Journal",
    preview_url: "https://images.steamusercontent.com/ugc/14380086584376552796/8391C9CC390A0FA349F1343BB1C03E3A5B397CC7/",
    description_excerpt: "Recover a configurable share of lost XP through a crafted journal.",
    description: "Create a personal journal that records learned skills and restores a configurable percentage after death. Server owners can control recovery limits and crafting rules.",
    creator_id: "synthetic-creator-0005",
    file_size: 2548731,
    created_at_unix: 1682899200,
    updated_at_unix: 1779494400,
    subscriptions: 612000,
    favorites: 36700,
    views: 940000,
    tags: ["Multiplayer", "Balance", "Quality of Life"],
    detail_url: "https://steamcommunity.com/sharedfiles/filedetails/?id=3000065999",
    item_kind: "item",
    status: "resolved",
    message: null,
    consumer_app_id: 108600,
    creator_app_id: 108600,
    child_count: 0,
    children: []
  }
};

export const mockDstModConfigurationSpecCatalog: Record<string, DstModConfigurationSpec> = {
  "1909182187": {
    mod_id: "1909182187",
    client_only: false,
    mod_dir: "D:/LanGame/server-files/dontstarve/mods/workshop-1909182187",
    modinfo_path: "D:/LanGame/server-files/dontstarve/mods/workshop-1909182187/modinfo.lua",
    mod_name: "Insight",
    description: "Adds inspect overlays and quality-of-life server data.",
    status: "loaded",
    message: null,
    options: [
      {
        name: "language",
        label: "Language",
        hover: "Choose the language used by the Insight overlay.",
        default_value: { kind: "string", value: "auto" },
        options: [
          { label: "Default", value: { kind: "default" } },
          { label: "Auto", value: { kind: "string", value: "auto" } },
          { label: "Chinese", value: { kind: "string", value: "zh" } },
          { label: "English", value: { kind: "string", value: "en" } }
        ]
      },
      {
        name: "show_creature_age",
        label: "Show Creature Age",
        hover: "Show age and growth stage on inspectable targets.",
        default_value: { kind: "boolean", value: true },
        options: [
          { label: "Default", value: { kind: "default" } },
          { label: "Enabled", value: { kind: "boolean", value: true } },
          { label: "Disabled", value: { kind: "boolean", value: false } }
        ]
      }
    ]
  },
  "2039181790": {
    mod_id: "2039181790",
    client_only: false,
    mod_dir: "D:/LanGame/server-files/dontstarve/mods/workshop-2039181790",
    modinfo_path: "D:/LanGame/server-files/dontstarve/mods/workshop-2039181790/modinfo.lua",
    mod_name: "Global Positions",
    description: "Shared map pings and world markers.",
    status: "loaded",
    message: null,
    options: [
      {
        name: "language",
        label: "Language",
        hover: "Controls the display language for markers and buttons.",
        default_value: { kind: "string", value: "en" },
        options: [
          { label: "Default", value: { kind: "default" } },
          { label: "Chinese", value: { kind: "string", value: "zh" } },
          { label: "English", value: { kind: "string", value: "en" } }
        ]
      },
      {
        name: "marker_scale",
        label: "Marker Scale",
        hover: "Controls the scale multiplier for map markers.",
        default_value: { kind: "number", value: 1 },
        options: [
          { label: "Default", value: { kind: "default" } },
          { label: "1.0x", value: { kind: "number", value: 1 } },
          { label: "1.5x", value: { kind: "number", value: 1.5 } },
          { label: "2.0x", value: { kind: "number", value: 2 } }
        ]
      },
      {
        name: "range_ring",
        label: "Range Ring",
        hover: "Show range hints around interactable objects.",
        default_value: { kind: "boolean", value: true },
        options: [
          { label: "Default", value: { kind: "default" } },
          { label: "Enabled", value: { kind: "boolean", value: true } },
          { label: "Disabled", value: { kind: "boolean", value: false } }
        ]
      }
    ]
  }
};


export function assistantSecretKey(descriptor: AssistantSecretDescriptor) {
  return `${String(descriptor.provider ?? "").trim().toLowerCase()}|${String(descriptor.baseUrl ?? "").trim().replace(/\/+$/, "").toLowerCase()}`;
}
export function parseSettingsJson(settingsJson: string): Record<string, unknown> | null {
  try {
    return JSON.parse(settingsJson) as Record<string, unknown>;
  } catch {
    return null;
  }
}

export function defaultMockRuntimePerformancePolicy(): RuntimePerformancePolicy {
  return {
    resource_limits: { cpu_percent: null, memory_limit_mib: null, host_memory_reserve_mib: 2048 },
    priority_class: "above_normal",
    cpu_affinity_mask: null,
    apply_to_child_processes: true,
    startup_stagger_ms: 1500,
    child_process_stagger_ms: 500
  };
}

function readMockRuntimeSettings(settings: Record<string, unknown>): Record<string, unknown> {
  const nested = settings.runtime_performance;
  return typeof nested === "object" && nested !== null && !Array.isArray(nested)
    ? nested as Record<string, unknown>
    : {};
}

function readMockNumberSetting(
  settings: Record<string, unknown>,
  runtimeSettings: Record<string, unknown>,
  key: string,
  fallback: number
): number {
  const value = runtimeSettings[key] ?? settings[`runtime_${key}`];
  const numericValue = typeof value === "number" ? value : typeof value === "string" ? Number(value) : Number.NaN;
  return Number.isFinite(numericValue) ? numericValue : fallback;
}

function readMockBooleanSetting(
  settings: Record<string, unknown>,
  runtimeSettings: Record<string, unknown>,
  key: string,
  fallback: boolean
): boolean {
  const value = runtimeSettings[key] ?? settings[`runtime_${key}`];
  return typeof value === "boolean" ? value : fallback;
}

function parseMockAffinityMask(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return Math.trunc(value);
  }
  if (typeof value !== "string") {
    return null;
  }

  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = trimmed.toLowerCase().startsWith("0x")
    ? Number.parseInt(trimmed.slice(2), 16)
    : Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function mockAffinityMaskForRange(start: number, end: number): number {
  let mask = 0;
  for (let index = Math.max(0, start); index < Math.min(8, end); index += 1) {
    mask += 2 ** index;
  }
  return mask;
}

function mockBalancedAffinityMask(instanceId: string): number {
  const buckets = [0x03, 0x0C, 0x30, 0xC0];
  let hash = 0;
  for (const char of instanceId) {
    hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  }
  return buckets[hash % buckets.length];
}

interface MockAffinityPresetResolution {
  preset: string;
  mask: number | null;
}

function resolveMockAffinityPreset(value: unknown, instanceId: string): MockAffinityPresetResolution | null {
  if (typeof value !== "string") {
    return null;
  }

  const preset = value.trim().toLowerCase().replace(/-/g, "_");
  switch (preset) {
    case "all":
    case "all_cpus":
      return { preset, mask: null };
    case "host_reserve":
    case "reserve_first_core":
      return { preset, mask: 0xFE };
    case "first_half":
      return { preset, mask: mockAffinityMaskForRange(0, 4) };
    case "second_half":
      return { preset, mask: mockAffinityMaskForRange(4, 8) };
    case "multi_instance_balance":
    case "balanced_multi_instance":
    case "auto_balance":
      return { preset, mask: mockBalancedAffinityMask(instanceId) };
    default:
      return null;
  }
}

export function resolveMockRuntimePerformancePolicy(
  settings: Record<string, unknown>,
  instanceId: string,
  modulePolicy: RuntimePerformancePolicy = defaultMockRuntimePerformancePolicy()
): RuntimePerformancePolicy {
  const runtimeSettings = readMockRuntimeSettings(settings);
  const priorityClass = runtimeSettings.priority_class ?? settings.runtime_priority_class ?? modulePolicy.priority_class;
  const explicitAffinityMask = parseMockAffinityMask(
    runtimeSettings.cpu_affinity_mask ?? settings.runtime_cpu_affinity_mask
  );
  const presetAffinity = resolveMockAffinityPreset(
    runtimeSettings.cpu_affinity_preset ?? settings.runtime_cpu_affinity_preset,
    instanceId
  );
  const cpuAffinityMask = explicitAffinityMask !== null
    ? explicitAffinityMask
    : presetAffinity
      ? presetAffinity.mask
      : modulePolicy.cpu_affinity_mask ?? null;

  return {
    priority_class: String(priorityClass) as RuntimePerformancePolicy["priority_class"],
    resource_limits: readResourceLimits(settings),
    cpu_affinity_mask: cpuAffinityMask,
    apply_to_child_processes: readMockBooleanSetting(
      settings,
      runtimeSettings,
      "apply_to_child_processes",
      modulePolicy.apply_to_child_processes
    ),
    startup_stagger_ms: readMockNumberSetting(
      settings,
      runtimeSettings,
      "startup_stagger_ms",
      modulePolicy.startup_stagger_ms
    ),
    child_process_stagger_ms: readMockNumberSetting(
      settings,
      runtimeSettings,
      "child_process_stagger_ms",
      modulePolicy.child_process_stagger_ms
    )
  };
}

export function buildMockRuntimePerformancePolicyPreview(
  settings: Record<string, unknown>,
  instanceId: string,
  policy: RuntimePerformancePolicy,
  modulePolicy: RuntimePerformancePolicy = defaultMockRuntimePerformancePolicy()
): RuntimePerformancePolicyPreview {
  const runtimeSettings = readMockRuntimeSettings(settings);
  const explicitAffinityMask = parseMockAffinityMask(
    runtimeSettings.cpu_affinity_mask ?? settings.runtime_cpu_affinity_mask
  );
  const presetAffinity = resolveMockAffinityPreset(
    runtimeSettings.cpu_affinity_preset ?? settings.runtime_cpu_affinity_preset,
    instanceId
  );
  const prioritySource =
    runtimeSettings.priority_class ?? settings.runtime_priority_class
      ? "instance_override"
      : "module_default";
  const cpuAffinitySource = explicitAffinityMask !== null
    ? "instance_mask"
    : presetAffinity
      ? "instance_preset"
      : modulePolicy.cpu_affinity_mask
        ? "module_default"
        : "all_cpus";
  const affinity = policy.cpu_affinity_mask
    ? `0x${Math.trunc(policy.cpu_affinity_mask).toString(16).toUpperCase()}`
    : "all CPUs";
  const presetLabel = presetAffinity ? `, preset=${presetAffinity.preset}` : "";

  return {
    summary: `Runtime performance policy resolves to priority=${policy.priority_class} (${prioritySource}), affinity=${affinity} (${cpuAffinitySource}${presetLabel}) on 8 logical CPU(s), instance stagger=${policy.startup_stagger_ms}ms, child process stagger=${policy.child_process_stagger_ms}ms.`,
    priority_source: prioritySource,
    cpu_affinity_source: cpuAffinitySource,
    cpu_affinity_preset: presetAffinity?.preset ?? null,
    logical_cpu_count: 8
  };
}

const MOCK_NECESSE_PERMISSION_LEVELS = ["USER", "MODERATOR", "ADMIN", "OWNER"] as const;

type MockNecessePermissionLevel = (typeof MOCK_NECESSE_PERMISSION_LEVELS)[number];

interface MockNecesseConsoleState {
  bans: string[];
  permissions: Record<string, MockNecessePermissionLevel>;
}

interface MockNecesseIdentityRecord {
  name: string;
  authentication: string;
}

const mockNecesseConsoleStateStore = new Map<string, MockNecesseConsoleState>();

function normalizeMockNecesseIdentity(value: string): string {
  return String(value ?? "").trim();
}

function slugMockNecesseIdentity(value: string): string {
  return normalizeMockNecesseIdentity(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    || "player";
}

function toMockNecesseIdentityRecord(value: string): MockNecesseIdentityRecord {
  const normalized = normalizeMockNecesseIdentity(value);
  if (!normalized) {
    return {
      name: "UnknownPlayer",
      authentication: "auth-unknown-player"
    };
  }

  if (/^auth[-_]/i.test(normalized)) {
    return {
      name: normalized,
      authentication: normalized
    };
  }

  return {
    name: normalized,
    authentication: `auth-${slugMockNecesseIdentity(normalized)}`
  };
}

function formatMockNecessePermissionLevel(level: MockNecessePermissionLevel): string {
  return `${level.charAt(0)}${level.slice(1).toLowerCase()}`;
}

function resolveMockNecesseOwnerName(details: InstanceDetails): string {
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const owner = typeof settings.owner_name === "string" ? settings.owner_name.trim() : "";
  return owner || "ChangeMeOwner";
}

function ensureMockNecesseConsoleState(instanceId: string, details: InstanceDetails): MockNecesseConsoleState {
  const cached = mockNecesseConsoleStateStore.get(instanceId);
  if (cached) {
    return cached;
  }

  const ownerName = resolveMockNecesseOwnerName(details);
  const initialState: MockNecesseConsoleState = {
    bans: [],
    permissions: ownerName ? { [ownerName]: "OWNER" } : {}
  };
  mockNecesseConsoleStateStore.set(instanceId, initialState);
  return initialState;
}

function buildMockNecesseKnownPlayers(details: InstanceDetails, state: MockNecesseConsoleState): MockNecesseIdentityRecord[] {
  const knownEntries = [
    resolveMockNecesseOwnerName(details),
    "BuilderBee",
    "NightScout",
    ...Object.keys(state.permissions),
    ...state.bans
  ];
  const seen = new Set<string>();

  return knownEntries
    .map(toMockNecesseIdentityRecord)
    .filter((entry) => {
      const key = entry.authentication.toLowerCase();
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });
}

export function buildMockNecesseCommandLines(details: InstanceDetails, commandText: string): string[] {
  const trimmedCommand = commandText.trim();
  if (!trimmedCommand) {
    return [];
  }

  const state = ensureMockNecesseConsoleState(details.summary.id, details);
  const commandParts = trimmedCommand.split(/\s+/).filter(Boolean);
  const commandName = commandParts[0]?.toLowerCase() ?? "";

  switch (commandName) {
    case "players": {
      const onlinePlayers = buildMockNecesseKnownPlayers(details, state).slice(0, 1);
      if (onlinePlayers.length === 0) {
        return ["Players online: 0"];
      }
      return [
        `Players online: ${onlinePlayers.length}`,
        ...onlinePlayers.map((player, index) => `${index + 1}. ${player.name} (${player.authentication})`)
      ];
    }
    case "playernames": {
      const storedPlayers = buildMockNecesseKnownPlayers(details, state);
      return [
        `Total players stored: ${storedPlayers.length}`,
        ...storedPlayers.map((player) => `${player.authentication} -> ${player.name}`)
      ];
    }
    case "permissions": {
      const action = commandParts[1]?.toLowerCase() ?? "";
      if (action === "list") {
        return [
          "Permission levels:",
          "User, Creative Settings, Moderator, Admin, Owner, Server"
        ];
      }

      if (action === "get") {
        const target = normalizeMockNecesseIdentity(commandParts.slice(2).join(" "));
        if (!target) {
          return ["permissions get <authentication/name>"];
        }
        const level = state.permissions[target] ?? "USER";
        return [`${target} currently has ${formatMockNecessePermissionLevel(level)} permissions.`];
      }

      if (action === "set") {
        const rawLevel = commandParts[commandParts.length - 1]?.toUpperCase() ?? "";
        const target = normalizeMockNecesseIdentity(commandParts.slice(2, -1).join(" "));
        if (!target || !MOCK_NECESSE_PERMISSION_LEVELS.includes(rawLevel as MockNecessePermissionLevel)) {
          return ["permissions set <authentication/name> <permission level>"];
        }
        state.permissions[target] = rawLevel as MockNecessePermissionLevel;
        return [`Set permissions of ${target} to ${formatMockNecessePermissionLevel(state.permissions[target])}.`];
      }

      return ["permissions list|get|set"];
    }
    case "ban": {
      const target = normalizeMockNecesseIdentity(commandParts.slice(1).join(" "));
      if (!target) {
        return ["ban <authentication/name>"];
      }
      if (state.bans.includes(target)) {
        return [`${target} is already banned.`];
      }
      state.bans = [...state.bans, target];
      return [`Banned ${target}`];
    }
    case "unban": {
      const target = normalizeMockNecesseIdentity(commandParts.slice(1).join(" "));
      if (!target) {
        return ["unban <authentication/name>"];
      }
      if (!state.bans.includes(target)) {
        return [`${target} is not banned.`];
      }
      state.bans = state.bans.filter((entry) => entry !== target);
      return [`${target} is no longer banned.`];
    }
    case "bans": {
      if (state.bans.length === 0) {
        return ["There are no listed bans."];
      }
      return [
        `${state.bans.length} total bans:`,
        ...state.bans.map((entry, index) => `${index + 1}. ${entry}`)
      ];
    }
    case "save":
      return ["Starting save...", "Save completed."];
    default:
      return [];
  }
}

export function normalizeAssistantEndpoint(baseUrl: string) {
  const trimmed = String(baseUrl ?? "").trim().replace(/\/+$/, "");
  if (!trimmed) {
    return "/chat/completions";
  }
  return trimmed.endsWith("/chat/completions") ? trimmed : `${trimmed}/chat/completions`;
}

export function buildMockAssistantResponse(input: AssistantRunInput): string {
  const context = input.context.toLowerCase();
  const evidence: string[] = [];
  const steps: string[] = [];
  let conclusion = "The current snapshot looks usable, but it still needs one targeted validation pass.";

  if (context.includes("storage ready: false")) {
    conclusion = "Storage is the primary blocker right now, so diagnosis will stay incomplete until it is initialized.";
    evidence.push("The context says storage is not ready, so runtime history and persisted records are incomplete.");
    steps.push("Open the System page and initialize storage first.");
  }

  if (context.includes("ai ready: false")) {
    evidence.push("BYOK is not fully ready yet, so this run is falling back to mock-mode reasoning only.");
    steps.push("Finish the BYOK fields in the header Settings panel before trusting live model output.");
  }

  if (context.includes("executable exists: false")) {
    conclusion = "The launch chain is blocked by a missing executable, so this is not a pure network problem.";
    evidence.push("Launch Preview reports that the executable path does not exist.");
    steps.push("Return to the Library page and make sure the game install completed successfully.");
  }

  if (context.includes("bind ip: 127.0.0.1")) {
    conclusion = "The instance is likely bound to localhost, so remote players will not be able to connect.";
    evidence.push("Selected Instance shows Bind IP as 127.0.0.1.");
    steps.push("Change bind_ip to 0.0.0.0 or a reachable LAN / overlay address.");
  }

  if (context.includes("summary: the server is running") || context.includes("status: ready")) {
    evidence.push("The runtime snapshot suggests the process is up, so focus on connectivity configuration before restart loops.");
  }

  if (steps.length === 0) {
    steps.push("Review the selected prompt result against the latest console and launch preview.");
    steps.push("If the issue is still unclear, rerun the prompt after refreshing the console so the context is fresher.");
  }

  if (evidence.length === 0) {
    evidence.push("The mock path only sees the packaged context, so it cannot confirm anything beyond the provided snapshot.");
  }

  return [
    `Conclusion: ${conclusion}`,
    "",
    "Evidence:",
    ...evidence.map((item) => `- ${item}`),
    "",
    "Next steps:",
    ...steps.map((item) => `- ${item}`),
    "",
    `Prompt label: ${input.promptLabel}`
  ].join("\n");
}
export function lookupMockWorkshopItems(ids: string[]): SteamWorkshopLookupItem[] {
  return ids.map((id) => {
    const known = mockWorkshopLookupCatalog[id];
    if (known) {
      return clone(known);
    }

    return {
      id,
      title: null,
      preview_url: null,
      description_excerpt: null,
      detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}`,
      item_kind: "item",
      status: "not_found",
      message: "Mock mode could not resolve this Workshop ID.",
      consumer_app_id: null,
      creator_app_id: null,
      child_count: 0,
      children: []
    };
  });
}

export function searchMockWorkshopItems(appId: number, query: string, sort: string, page: number, browseKind: SteamWorkshopBrowseKind = "item"): SteamWorkshopSearchResult {
  const normalizedQuery = query.trim().toLowerCase();
  const items = Object.values(mockWorkshopLookupCatalog)
    .filter((item) => item.consumer_app_id === appId && item.item_kind === browseKind && item.status === "resolved")
    .filter((item) => !normalizedQuery || `${item.id} ${item.title ?? ""} ${item.description_excerpt ?? ""}`.toLowerCase().includes(normalizedQuery));

  return {
    app_id: appId,
    browse_kind: browseKind,
    query,
    sort,
    page,
    source_url: `https://steamcommunity.com/workshop/browse/?appid=${appId}&section=${browseKind === "collection" ? "collections" : "readytouseitems"}`,
    page_size: 30,
    total_count: items.length,
    has_more: page * 30 < items.length,
    items: clone(items.slice((page - 1) * 30, page * 30))
  };
}

export function readMockDstModConfigurationSpecs(ids: string[]): DstModConfigurationSpec[] {
  return ids.map((id) => {
    const normalized = String(id).trim().replace(/^workshop-/, "");
    const known = mockDstModConfigurationSpecCatalog[normalized];
    if (known) {
      return clone(known);
    }

    return {
      mod_id: normalized,
      client_only: false,
      mod_dir: `D:/LanGame/server-files/dontstarve/mods/workshop-${normalized}`,
      modinfo_path: `D:/LanGame/server-files/dontstarve/mods/workshop-${normalized}/modinfo.lua`,
      mod_name: null,
      description: null,
      status: "missing_mod",
      message: "Mock mode has no local modinfo sample for this Workshop ID yet.",
      options: []
    };
  });
}

export function readMockProjectZomboidWorkshopModsSnapshot(ids: string[]): ProjectZomboidWorkshopModsSnapshot {
  const normalizedIds = Array.from(
    new Set(
      ids
        .map((id) => String(id).trim())
        .filter((id) => id.length > 0)
    )
  );

  return {
    workshop_root: "D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600",
    workshop_root_exists: true,
    items: normalizedIds.map((id) => {
      if (id === "2945221351") {
        return {
          workshop_item_id: id,
          item_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}`,
          mods_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods`,
          status: "installed",
          message: null,
          mods: [
            {
              directory_name: "RavenCreek",
              mod_id: "RavenCreek",
              mod_name: "Raven Creek",
              mod_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods/RavenCreek`,
              mod_info_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods/RavenCreek/mod.info`,
              map_ids: ["RavenCreek"],
              status: "loaded",
              message: null
            }
          ]
        };
      }

      if (id === "3000065999") {
        return {
          workshop_item_id: id,
          item_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}`,
          mods_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods`,
          status: "installed_with_warnings",
          message: "One local mod was detected, but its mod.info still needs review.",
          mods: [
            {
              directory_name: "SkillRecoveryJournal",
              mod_id: "SkillRecoveryJournal",
              mod_name: "Skill Recovery Journal",
              mod_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods/SkillRecoveryJournal`,
              mod_info_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods/SkillRecoveryJournal/mod.info`,
              map_ids: [],
              status: "loaded",
              message: null
            },
            {
              directory_name: "ReadMeAssets",
              mod_id: null,
              mod_name: null,
              mod_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}/Contents/mods/ReadMeAssets`,
              mod_info_path: null,
              map_ids: [],
              status: "missing_mod_info",
              message: "No mod.info file was found in this local mod folder or its version subfolders."
            }
          ]
        };
      }

      return {
        workshop_item_id: id,
        item_path: `D:/LanGame/server-files/projectzomboid/steamapps/workshop/content/108600/${id}`,
        mods_path: null,
        status: "missing_item",
        message: "Mock mode has no downloaded Project Zomboid Workshop sample for this ID yet.",
        mods: []
      };
    })
  };
}
