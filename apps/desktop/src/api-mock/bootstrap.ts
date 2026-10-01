import type { BindAddressCandidate, BootstrapResponse, OverlayFamily } from "../types";
import { buildMockModuleSummaries } from "./module-assets";

const mockInstances: BootstrapResponse["state"]["instances"] = [
  {
    id: "srv-dst-1",
    name: "DST Friday Night",
    module_id: "dontstarve",
    status: "Running",
    active_process_count: 2,
    bind_ip: "0.0.0.0",
    port_count: 2,
    autostart: true
  },
  {
    id: "srv-dst-partial-error",
    name: "DST Partial Shard Failure",
    module_id: "dontstarve",
    status: "Error",
    active_process_count: 1,
    bind_ip: "0.0.0.0",
    port_count: 2,
    autostart: false
  },
  {
    id: "srv-dst-terminal-error",
    name: "DST Fully Stopped Error",
    module_id: "dontstarve",
    status: "Error",
    active_process_count: 0,
    bind_ip: "0.0.0.0",
    port_count: 2,
    autostart: false
  },
  {
    id: "srv-minecraft-1",
    name: "Minecraft Build Night",
    module_id: "minecraft",
    status: "Running",
    active_process_count: 1,
    bind_ip: "0.0.0.0",
    port_count: 2,
    autostart: false
  },
  {
    id: "srv-ark-island",
    name: "ASA The Island",
    module_id: "arksurvivalascended",
    status: "Stopped",
    active_process_count: 0,
    bind_ip: "0.0.0.0",
    port_count: 4,
    autostart: false
  },
  {
    id: "srv-ase-island",
    name: "ASE The Island",
    module_id: "arksurvivalevolved",
    status: "Stopped",
    active_process_count: 0,
    bind_ip: "0.0.0.0",
    port_count: 4,
    autostart: false
  },
  {
    id: "srv-corekeeper-1",
    name: "Core Keeper Weekend",
    module_id: "corekeeper",
    status: "Stopped",
    active_process_count: 0,
    bind_ip: "0.0.0.0",
    port_count: 1,
    autostart: false
  },
  {
    id: "srv-necesse-1",
    name: "Necesse Night Shift",
    module_id: "necesse",
    status: "Stopped",
    active_process_count: 0,
    bind_ip: "0.0.0.0",
    port_count: 1,
    autostart: false
  }
];

export const mockBootstrap: BootstrapResponse = {
  booted_at_unix_ms: Date.now(),
  state: {
    settings: {
      servers_root: "D:/LanGame/instances",
      games_root: "D:/LanGame/server-files",
      archives_root: "D:/LanGame/instances/.trash",
      modules_root: "./modules",
      steamcmd_root: "D:/LanGame/cmd/steamcmd"
    },
    storage: {
      database_path: "LocalAppData/LanGame/ServerManager/db/lgs.db",
      migrations_path: "./migrations",
      app_log_path: "LocalAppData/LanGame/ServerManager/logs/desktop-app.log",
      database_exists: false,
      schema_version: 0,
      migrations_applied: false
    },
    modules: buildMockModuleSummaries(),
    instances: mockInstances,
    jobs: [],
    snapshot: {
      telemetry: {
        observed_at_unix_ms: Date.now(),
        cpu: "valid",
        cpu_cores: "valid",
        memory: "valid",
        disk_capacity: "valid",
        disk_io: "valid",
        network: "valid"
      },
      memory_commit_used_bytes: 34359738368,
      memory_commit_limit_bytes: 103079215104,
      disk_volumes: [{
        id: "mock-volume-d",
        label: "D:\\",
        paths: ["D:/LanGame/server-files", "D:/LanGame/instances", "D:/LanGame/instances/.trash", "D:/LanGame/cmd/steamcmd"],
        total_bytes: 536870912000,
        available_bytes: 246960619520,
        free_bytes: 246960619520,
        status: "valid"
      }],
      cpu_percent: 18,
      cpu_name: "AMD Ryzen 9 9950X3D 16-Core Processor",
      cpu_frequency_mhz: 4292,
      cpu_max_frequency_mhz: 4300,
      cpu_physical_cores: 16,
      cpu_logical_cores: 32,
      cpu_single_core_peak_percent: 37,
      cpu_performance_percent: 112,
      cpu_cores: [
        { name: "0,0", utility_percent: 12, performance_percent: 121, frequency_mhz: 4300 },
        { name: "0,1", utility_percent: 37, performance_percent: 118, frequency_mhz: 4300 }
      ],
      memory_percent: 36,
      memory_total_bytes: 68719476736,
      memory_available_bytes: 43980465111,
      memory_modules: [
        {
          bank_label: "P0 CHANNEL A",
          device_locator: "DIMM 0",
          manufacturer: "CORSAIR",
          part_number: "CMK96GX5M2B5600C40",
          capacity_bytes: 51539607552,
          speed_mts: 3600,
          configured_clock_mts: 3600,
          configured_voltage_mv: 1100,
          memory_type: "DDR5",
          inferred_cas_latency: 40,
          timing_summary: "DDR5-3600 CL40"
        }
      ],
      disk_used_percent: 54,
      disk_used_bytes: 289910292480,
      disk_total_bytes: 536870912000,
      disk_label: "D:\\",
      disk_volume_id: "mock-volume-d",
      disk_model: "Preview SSD",
      disk_volume_name: "LanGame",
      disk_file_system: "NTFS",
      disk_read_bps: 4096,
      disk_write_bps: 196608,
      disk_read_latency_ms: 0.12,
      disk_write_latency_ms: 0.08,
      disk_queue_length: 0,
      network_receive_bps: 1048576,
      network_transmit_bps: 262144,
      network_adapters: [
        {
          name: "Radmin VPN",
          rate_status: "valid",
          description: "Famatech Radmin VPN Ethernet Adapter",
          status: "Up",
          family_name: "Radmin VPN",
          ipv4_addresses: ["198.51.100.218"],
          mac_address: "02:50:85:A7:B6:0E",
          link_speed_bps: 100000000,
          received_bytes: 0,
          transmitted_bytes: 0,
          receive_bps: 0,
          transmit_bps: 0
        },
        {
          name: "Ethernet",
          rate_status: "valid",
          description: "Realtek PCIe 2.5GbE Family Controller",
          status: "Up",
          family_name: null,
          ipv4_addresses: ["192.0.2.150"],
          mac_address: "02:00:00:00:00:02",
          link_speed_bps: 2500000000,
          received_bytes: 1725561647,
          transmitted_bytes: 1895831550,
          receive_bps: 1048576,
          transmit_bps: 262144
        }
      ],
      instance_process_memory_bytes: 2415919104,
      instance_process_memory_percent: 3.5,
      instance_process_count: 2,
      instance_process_threads: 128,
      instance_process_handles: 4200,
      running_instances: mockInstances.filter((instance) => instance.status === "Running").length,
      total_online_players: 3,
      total_player_capacity: 6,
      player_count_queried_instances: 1,
      player_count_queryable_instances: 1
    }
  }
};

export const mockOverlays: OverlayFamily[] = [
  {
    name: "ZeroTier One",
    adapter_name_patterns: ["ZeroTier", "ZeroTier One"],
    executable_name_patterns: ["zerotier-one.exe"]
  },
  {
    name: "Radmin VPN",
    adapter_name_patterns: ["Radmin", "Radmin VPN"],
    executable_name_patterns: ["rvpnsvc.exe"]
  }
];

export const mockBindAddressCandidates: BindAddressCandidate[] = [
  {
    address: "0.0.0.0",
    kind: "all",
    adapter_name: null,
    family_name: null
  },
  {
    address: "198.51.100.4",
    kind: "overlay",
    adapter_name: "Radmin VPN",
    family_name: "Radmin VPN"
  },
  {
    address: "192.0.2.42",
    kind: "lan",
    adapter_name: "Wi-Fi",
    family_name: null
  }
];
