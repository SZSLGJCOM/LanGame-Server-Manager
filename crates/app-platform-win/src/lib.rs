use app_core::{DiskVolumeSnapshot, SystemTelemetry, TelemetryStatus};
use app_core::{PortBinding, RuntimeWindowSurface};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::ffi::{OsStr, OsString};
use std::net::{IpAddr, Ipv4Addr};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod host_inventory;
#[cfg(test)]
mod host_monitor_tests;
mod host_telemetry;
mod process_inspection;
mod system_utility;
pub use process_inspection::WindowInspectionTarget;
use process_inspection::{TargetProcessRecord, collect_verified_target_process_records};
use system_utility::{FIREWALL_TIMEOUT, QUERY_TIMEOUT, SystemUtility};

const MAX_PATH_WIDE: usize = 260;
const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_VM_READ: u32 = 0x0010;
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_ELEVATION_CLASS: u32 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct OverlayFamily {
    pub name: String,
    pub adapter_name_patterns: Vec<String>,
    pub executable_name_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindAddressCandidate {
    pub address: String,
    pub kind: String,
    pub adapter_name: Option<String>,
    pub family_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct NetIpConfigurationRecord {
    #[serde(rename = "IPAddress")]
    ip_address: String,
    #[serde(rename = "InterfaceAlias")]
    interface_alias: Option<String>,
    #[serde(rename = "InterfaceDescription")]
    interface_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PhysicalMemoryRecord {
    #[serde(rename = "BankLabel")]
    bank_label: Option<String>,
    #[serde(rename = "DeviceLocator")]
    device_locator: Option<String>,
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<String>,
    #[serde(rename = "PartNumber")]
    part_number: Option<String>,
    #[serde(rename = "Capacity")]
    capacity: Option<u64>,
    #[serde(rename = "Speed")]
    speed: Option<u32>,
    #[serde(rename = "ConfiguredClockSpeed")]
    configured_clock_speed: Option<u32>,
    #[serde(rename = "ConfiguredVoltage")]
    configured_voltage: Option<u32>,
    #[serde(rename = "SMBIOSMemoryType")]
    smbios_memory_type: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessorCounterRecord {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "PercentProcessorUtility")]
    percent_processor_utility: Option<f32>,
    #[serde(rename = "PercentProcessorPerformance")]
    percent_processor_performance: Option<f32>,
    #[serde(rename = "ProcessorFrequency")]
    processor_frequency: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessorProfileRecord {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "MaxClockSpeed")]
    max_clock_speed: Option<u32>,
    #[serde(rename = "CurrentClockSpeed")]
    current_clock_speed: Option<u32>,
    #[serde(rename = "NumberOfCores")]
    number_of_cores: Option<u32>,
    #[serde(rename = "NumberOfLogicalProcessors")]
    number_of_logical_processors: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct NetworkAdapterRecord {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "LinkSpeedBps")]
    link_speed_bps: Option<u64>,
    #[serde(rename = "MacAddress")]
    mac_address: Option<String>,
    #[serde(rename = "IPv4Addresses")]
    ipv4_addresses: Option<Vec<String>>,
    #[serde(rename = "ReceivedBytes")]
    received_bytes: Option<u64>,
    #[serde(rename = "SentBytes")]
    sent_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct DiskCounterRecord {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "DiskReadBytesPersec")]
    disk_read_bytes_persec: Option<u64>,
    #[serde(rename = "DiskWriteBytesPersec")]
    disk_write_bytes_persec: Option<u64>,
    #[serde(rename = "CurrentDiskQueueLength")]
    current_disk_queue_length: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct WindowInspectionResult {
    pub inspected_process_count: usize,
    pub windows: Vec<RuntimeWindowSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessNetworkEndpoint {
    pub protocol: String,
    pub local_address: String,
    pub local_port: u16,
    pub owning_pid: u32,
    pub process_key: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessNetworkInspectionResult {
    pub inspected_process_count: usize,
    pub endpoints: Vec<ProcessNetworkEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedNetworkEndpoint {
    protocol: String,
    local_address: String,
    local_port: u16,
    owning_pid: u32,
}

#[derive(Debug, Clone)]
pub struct WindowSuppressionResult {
    pub inspected_process_count: usize,
    pub visible_window_count_before: usize,
    pub suppressed_window_count: usize,
    pub remaining_windows: Vec<RuntimeWindowSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowsFirewallRuleSpec {
    pub rule_name: String,
    pub protocol: String,
    pub local_port: u16,
    pub local_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsFirewallRuleApplyResult {
    pub rule_name: String,
    pub protocol: String,
    pub local_port: u16,
    pub local_address: String,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
struct ProcessSnapshotEntry {
    process_id: u32,
    parent_process_id: u32,
    process_name: String,
    thread_count: u32,
}

#[derive(Debug, Clone)]
struct ObservedWindowSurface {
    window_handle: usize,
    surface: RuntimeWindowSurface,
}

struct WindowEnumerationContext<'a> {
    processes: &'a HashMap<u32, TargetProcessRecord>,
    windows: Vec<ObservedWindowSurface>,
}

#[derive(Debug, Clone, Default)]
pub struct HostMetricsSnapshot {
    pub telemetry: SystemTelemetry,
    pub disk_volumes: Vec<DiskVolumeSnapshot>,
    pub memory_commit_used_bytes: Option<u64>,
    pub memory_commit_limit_bytes: Option<u64>,
    pub cpu_percent: f32,
    pub cpu_name: String,
    pub cpu_frequency_mhz: u32,
    pub cpu_max_frequency_mhz: u32,
    pub cpu_physical_cores: u32,
    pub cpu_logical_cores: u32,
    pub cpu_single_core_peak_percent: f32,
    pub cpu_performance_percent: f32,
    pub cpu_cores: Vec<CpuCoreMetrics>,
    pub memory_percent: f32,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub memory_modules: Vec<MemoryModuleMetrics>,
    pub disk_used_percent: f32,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_label: String,
    pub disk_volume_name: String,
    pub disk_volume_id: String,
    pub disk_model: String,
    pub disk_file_system: String,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub disk_read_latency_ms: f32,
    pub disk_write_latency_ms: f32,
    pub disk_queue_length: f32,
    pub network_receive_bps: u64,
    pub network_transmit_bps: u64,
    pub network_adapters: Vec<NetworkAdapterMetrics>,
}

#[derive(Debug, Clone, Default)]
pub struct CpuCoreMetrics {
    pub name: String,
    pub utility_percent: f32,
    pub performance_percent: f32,
    pub frequency_mhz: u32,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryModuleMetrics {
    pub bank_label: String,
    pub device_locator: String,
    pub manufacturer: String,
    pub part_number: String,
    pub capacity_bytes: u64,
    pub speed_mts: u32,
    pub configured_clock_mts: u32,
    pub configured_voltage_mv: u32,
    pub memory_type: String,
    pub inferred_cas_latency: Option<u32>,
    pub timing_summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkAdapterMetrics {
    pub rate_status: TelemetryStatus,
    pub name: String,
    pub description: String,
    pub status: String,
    pub family_name: Option<String>,
    pub ipv4_addresses: Vec<String>,
    pub mac_address: Option<String>,
    pub link_speed_bps: u64,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub receive_bps: u64,
    pub transmit_bps: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessMemoryMetrics {
    pub target_process_id: u32,
    pub process_id: u32,
    pub parent_process_id: u32,
    pub name: String,
    pub working_set_bytes: u64,
    pub page_file_usage_bytes: u64,
    pub thread_count: u32,
    pub handle_count: u32,
}

#[derive(Debug, Clone, Default)]
struct CpuProfile {
    name: String,
    frequency_mhz: u32,
    max_frequency_mhz: u32,
    physical_cores: u32,
    logical_cores: u32,
}

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

#[derive(Debug, Clone)]
struct NetworkSample {
    received: u64,
    transmitted: u64,
    sampled_at: Instant,
}

#[derive(Debug, Clone)]
struct NetworkAdapterSample {
    received: u64,
    transmitted: u64,
    sampled_at: Instant,
}

#[derive(Debug, Clone, Default)]
struct MemorySnapshot {
    load_percent: f32,
    total_bytes: u64,
    available_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct DiskSnapshot {
    io_status: TelemetryStatus,
    used_percent: f32,
    used_bytes: u64,
    total_bytes: u64,
    label: String,
    volume_name: String,
    file_system: String,
    read_bps: u64,
    write_bps: u64,
    read_latency_ms: f32,
    write_latency_ms: f32,
    queue_length: f32,
}

#[derive(Default)]
pub struct WindowsHostMonitor {
    initialized: bool,
    cpu_profile: CpuProfile,
    memory_modules: Vec<MemoryModuleMetrics>,
    disk_inventory: host_inventory::DiskInventoryCache,
    previous_cpu: Option<CpuTimes>,
    previous_network: Option<NetworkSample>,
    previous_network_adapters: HashMap<String, NetworkAdapterSample>,
    network_adapter_status: TelemetryStatus,
}

pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn current_process_is_elevated() -> Result<bool, String> {
        #[cfg(windows)]
        {
            query_current_process_elevation()
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    pub fn ensure_instance_firewall_rules(
        instance_id: &str,
        instance_name: &str,
        ports: &[PortBinding],
        local_address: Option<&str>,
    ) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
        ensure_instance_firewall_rules(instance_id, instance_name, ports, local_address)
    }

    pub fn supported_overlay_families() -> Vec<OverlayFamily> {
        vec![
            OverlayFamily {
                name: String::from("ZeroTier One"),
                adapter_name_patterns: vec![String::from("ZeroTier"), String::from("ZeroTier One")],
                executable_name_patterns: vec![
                    String::from("zerotier-one.exe"),
                    String::from("zerotier_desktop_ui.exe"),
                ],
            },
            OverlayFamily {
                name: String::from("Radmin VPN"),
                adapter_name_patterns: vec![String::from("Radmin"), String::from("Radmin VPN")],
                executable_name_patterns: vec![
                    String::from("rvpnsvc.exe"),
                    String::from("rvpnui.exe"),
                ],
            },
        ]
    }

    pub fn bind_address_candidates() -> Vec<BindAddressCandidate> {
        let mut candidates = vec![BindAddressCandidate {
            address: String::from("0.0.0.0"),
            kind: String::from("all"),
            adapter_name: None,
            family_name: None,
        }];
        candidates.extend(read_bind_address_candidates());
        candidates
    }

    pub fn inspect_process_network_endpoints(
        targets: &[WindowInspectionTarget],
        ports: &[u16],
    ) -> Result<ProcessNetworkInspectionResult, String> {
        inspect_process_network_endpoints(targets, ports)
    }

    pub fn inspect_visible_window_surfaces(
        targets: &[WindowInspectionTarget],
    ) -> Result<WindowInspectionResult, String> {
        let processes = collect_verified_target_process_records(targets)?;
        if processes.is_empty() {
            return Ok(WindowInspectionResult {
                inspected_process_count: 0,
                windows: Vec::new(),
            });
        }

        let windows = enumerate_visible_window_surfaces(&processes)?;
        Ok(WindowInspectionResult {
            inspected_process_count: processes.len(),
            windows,
        })
    }

    pub fn suppress_visible_window_surfaces(
        targets: &[WindowInspectionTarget],
    ) -> Result<WindowSuppressionResult, String> {
        let processes = collect_verified_target_process_records(targets)?;
        if processes.is_empty() {
            return Ok(WindowSuppressionResult {
                inspected_process_count: 0,
                visible_window_count_before: 0,
                suppressed_window_count: 0,
                remaining_windows: Vec::new(),
            });
        }

        let mut observed = enumerate_observed_window_surfaces(&processes)?;
        let visible_window_count_before = observed.len();
        let mut requested_handles = HashSet::<usize>::new();

        for _ in 0..2 {
            if observed.is_empty() {
                break;
            }

            for window in &observed {
                if requested_handles.contains(&window.window_handle) {
                    continue;
                }

                if hide_window_surface(window.window_handle, window.surface.pid) {
                    requested_handles.insert(window.window_handle);
                }
            }

            if requested_handles.is_empty() {
                break;
            }

            std::thread::sleep(Duration::from_millis(120));
            observed = enumerate_observed_window_surfaces(&processes)?;
        }

        let suppressed_window_count = confirmed_suppression_count(&requested_handles, &observed);
        let mut remaining_windows = observed
            .into_iter()
            .map(|window| window.surface)
            .collect::<Vec<_>>();
        sort_window_surfaces(&mut remaining_windows);

        Ok(WindowSuppressionResult {
            inspected_process_count: processes.len(),
            visible_window_count_before,
            suppressed_window_count,
            remaining_windows,
        })
    }
}

fn query_current_process_elevation() -> Result<bool, String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "open current process token: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut elevation = TokenElevation::default();
    let mut returned_length = 0;
    let query_succeeded = unsafe {
        GetTokenInformation(
            token,
            TOKEN_ELEVATION_CLASS,
            (&mut elevation as *mut TokenElevation).cast(),
            std::mem::size_of::<TokenElevation>() as u32,
            &mut returned_length,
        )
    } != 0;
    let query_error = (!query_succeeded).then(std::io::Error::last_os_error);
    unsafe {
        CloseHandle(token);
    }

    match query_error {
        Some(error) => Err(format!("inspect current process token elevation: {error}")),
        None => Ok(elevation.token_is_elevated != 0),
    }
}

impl WindowsHostMonitor {
    pub fn capture(&mut self, monitored_path: &str) -> HostMetricsSnapshot {
        self.capture_paths(monitored_path, &[monitored_path.to_string()])
    }

    pub fn capture_paths(
        &mut self,
        monitored_path: &str,
        storage_paths: &[String],
    ) -> HostMetricsSnapshot {
        let observed_at_unix_ms = host_telemetry::observation_time();
        // DesktopState is constructed on the UI thread. Delay external system
        // queries until capture runs inside its existing blocking worker.
        if !self.initialized {
            self.cpu_profile = read_cpu_profile();
            self.memory_modules = read_memory_modules();
            self.initialized = true;
        }
        let selected_volume = host_telemetry::resolve_volume_identity(Path::new(monitored_path));
        let disk = selected_volume
            .as_ref()
            .map(|(id, mount)| read_disk_snapshot(id, mount))
            .unwrap_or_default();
        let disk_model = self
            .disk_inventory
            .model_for_volume(selected_volume.as_ref().map(|(id, _)| id.as_str()));
        let memory = read_memory_snapshot();
        let (memory_commit_used_bytes, memory_commit_limit_bytes) =
            host_telemetry::read_commit_memory();
        let disk_volumes = host_telemetry::read_disk_volumes(storage_paths);
        let (cpu_cores, cpu_cores_status) = read_cpu_core_metrics();
        let memory_modules = self.memory_modules.clone();
        let network_adapters = self.sample_network_adapter_rates();
        let (network_receive_bps, network_transmit_bps, network_status) =
            self.sample_network_rates();
        let (cpu_percent, cpu_status) = self.sample_cpu_percent();
        let cpu_single_core_peak_percent = cpu_cores
            .iter()
            .map(|core| core.utility_percent)
            .fold(0.0_f32, f32::max)
            .clamp(0.0, 100.0);
        let cpu_performance_percent = if cpu_cores.is_empty() {
            0.0
        } else {
            cpu_cores
                .iter()
                .map(|core| core.performance_percent)
                .sum::<f32>()
                / cpu_cores.len() as f32
        };

        HostMetricsSnapshot {
            telemetry: SystemTelemetry {
                observed_at_unix_ms,
                cpu: cpu_status,
                cpu_cores: cpu_cores_status,
                memory: if memory.total_bytes > 0 {
                    TelemetryStatus::Valid
                } else {
                    TelemetryStatus::Unavailable
                },
                disk_capacity: if !disk_volumes.is_empty()
                    && disk_volumes
                        .iter()
                        .all(|volume| volume.status == TelemetryStatus::Valid)
                {
                    TelemetryStatus::Valid
                } else {
                    TelemetryStatus::Unavailable
                },
                disk_io: disk.io_status,
                network: host_telemetry::combined_network_status(
                    self.network_adapter_status,
                    network_status,
                ),
            },
            disk_volumes,
            memory_commit_used_bytes,
            memory_commit_limit_bytes,
            cpu_percent,
            cpu_name: self.cpu_profile.name.clone(),
            cpu_frequency_mhz: self.cpu_profile.frequency_mhz,
            cpu_max_frequency_mhz: self.cpu_profile.max_frequency_mhz,
            cpu_physical_cores: self.cpu_profile.physical_cores,
            cpu_logical_cores: self.cpu_profile.logical_cores,
            cpu_single_core_peak_percent,
            cpu_performance_percent,
            cpu_cores,
            memory_percent: memory.load_percent,
            memory_total_bytes: memory.total_bytes,
            memory_available_bytes: memory.available_bytes,
            memory_modules,
            disk_used_percent: disk.used_percent,
            disk_used_bytes: disk.used_bytes,
            disk_total_bytes: disk.total_bytes,
            disk_label: disk.label,
            disk_volume_name: disk.volume_name,
            disk_volume_id: selected_volume.map(|(id, _)| id).unwrap_or_default(),
            disk_model,
            disk_file_system: disk.file_system,
            disk_read_bps: disk.read_bps,
            disk_write_bps: disk.write_bps,
            disk_read_latency_ms: disk.read_latency_ms,
            disk_write_latency_ms: disk.write_latency_ms,
            disk_queue_length: disk.queue_length,
            network_receive_bps: network_adapters
                .iter()
                .map(|adapter| adapter.receive_bps)
                .sum::<u64>()
                .max(network_receive_bps),
            network_transmit_bps: network_adapters
                .iter()
                .map(|adapter| adapter.transmit_bps)
                .sum::<u64>()
                .max(network_transmit_bps),
            network_adapters,
        }
    }

    fn sample_cpu_percent(&mut self) -> (f32, TelemetryStatus) {
        let current = read_cpu_times();
        let result = host_telemetry::cpu_usage(self.previous_cpu, current);
        self.previous_cpu = current;
        result
    }

    fn sample_network_rates(&mut self) -> (u64, u64, TelemetryStatus) {
        let sampled_at = Instant::now();
        let Some((received, transmitted)) = read_network_totals() else {
            self.previous_network = None;
            return (0, 0, TelemetryStatus::Unavailable);
        };

        let rates = host_telemetry::counter_rates(
            self.previous_network
                .as_ref()
                .map(|previous| (previous.received, previous.transmitted, previous.sampled_at)),
            (received, transmitted),
            sampled_at,
        );

        self.previous_network = Some(NetworkSample {
            received,
            transmitted,
            sampled_at,
        });
        rates
    }

    fn sample_network_adapter_rates(&mut self) -> Vec<NetworkAdapterMetrics> {
        self.sample_network_adapter_records(read_network_adapter_records())
    }

    fn sample_network_adapter_records(
        &mut self,
        records: Vec<NetworkAdapterRecord>,
    ) -> Vec<NetworkAdapterMetrics> {
        let sampled_at = Instant::now();
        let families = WindowsPlatform::supported_overlay_families();
        let mut adapters = Vec::new();
        // Keep only the current inventory. Removed or renamed adapters must not
        // accumulate forever, and a returning adapter starts a fresh baseline.
        let previous_adapters = std::mem::take(&mut self.previous_network_adapters);
        self.network_adapter_status = TelemetryStatus::Valid;
        let mut has_active_adapter = false;

        for record in records {
            let is_active = record
                .status
                .as_deref()
                .is_some_and(|status| status.trim().eq_ignore_ascii_case("up"));
            has_active_adapter |= is_active;
            let name = normalize_whitespace(record.name.as_deref().unwrap_or_default());
            if name.is_empty() {
                if is_active {
                    self.network_adapter_status = TelemetryStatus::Unavailable;
                }
                continue;
            }

            let description =
                normalize_whitespace(record.description.as_deref().unwrap_or_default());
            let received = record.received_bytes.unwrap_or(0);
            let transmitted = record.sent_bytes.unwrap_or(0);
            let rates = if record.received_bytes.is_some() && record.sent_bytes.is_some() {
                host_telemetry::counter_rates(
                    previous_adapters.get(&name).map(|previous| {
                        (previous.received, previous.transmitted, previous.sampled_at)
                    }),
                    (received, transmitted),
                    sampled_at,
                )
            } else {
                (0, 0, TelemetryStatus::Unavailable)
            };
            if is_active
                && (rates.2 == TelemetryStatus::Unavailable
                    || (rates.2 == TelemetryStatus::WarmingUp
                        && self.network_adapter_status == TelemetryStatus::Valid))
            {
                self.network_adapter_status = rates.2;
            }

            if rates.2 != TelemetryStatus::Unavailable {
                self.previous_network_adapters.insert(
                    name.clone(),
                    NetworkAdapterSample {
                        received,
                        transmitted,
                        sampled_at,
                    },
                );
            }

            let family_name =
                overlay_family_for_adapter(Some(&name), Some(&description), &families)
                    .map(|family| family.name.clone());

            adapters.push(NetworkAdapterMetrics {
                rate_status: rates.2,
                name,
                description,
                status: normalize_whitespace(record.status.as_deref().unwrap_or_default()),
                family_name,
                ipv4_addresses: record.ipv4_addresses.unwrap_or_default(),
                mac_address: record
                    .mac_address
                    .map(|value| normalize_whitespace(&value))
                    .filter(|value| !value.is_empty()),
                link_speed_bps: record.link_speed_bps.unwrap_or(0),
                received_bytes: received,
                transmitted_bytes: transmitted,
                receive_bps: rates.0,
                transmit_bps: rates.1,
            });
        }

        if !has_active_adapter {
            self.network_adapter_status = TelemetryStatus::Unavailable;
        }

        adapters.sort_by(|left, right| {
            network_adapter_priority(left)
                .cmp(&network_adapter_priority(right))
                .then_with(|| left.name.cmp(&right.name))
        });
        adapters
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    dw_low_date_time: u32,
    dw_high_date_time: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryStatusEx {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WindowRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct TokenElevation {
    token_is_elevated: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemTimes(
        idle_time: *mut FileTime,
        kernel_time: *mut FileTime,
        user_time: *mut FileTime,
    ) -> i32;
    fn GlobalMemoryStatusEx(memory_status: *mut MemoryStatusEx) -> i32;
    fn GetDiskFreeSpaceExW(
        directory_name: *const u16,
        free_bytes_available_to_caller: *mut u64,
        total_number_of_bytes: *mut u64,
        total_number_of_free_bytes: *mut u64,
    ) -> i32;
    fn GetVolumeInformationW(
        root_path_name: *const u16,
        volume_name_buffer: *mut u16,
        volume_name_size: u32,
        volume_serial_number: *mut u32,
        maximum_component_length: *mut u32,
        file_system_flags: *mut u32,
        file_system_name_buffer: *mut u16,
        file_system_name_size: u32,
    ) -> i32;
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessHandleCount(process_handle: *mut c_void, handle_count: *mut u32) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(
        process_handle: *mut c_void,
        desired_access: u32,
        token_handle: *mut *mut c_void,
    ) -> i32;
    fn GetTokenInformation(
        token_handle: *mut c_void,
        token_information_class: u32,
        token_information: *mut c_void,
        token_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut ProcessMemoryCountersEx,
        size: u32,
    ) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(window: *mut c_void, process_id: *mut u32) -> u32;
    fn IsWindowVisible(window: *mut c_void) -> i32;
    fn GetWindowRect(window: *mut c_void, rect: *mut WindowRect) -> i32;
    fn GetWindowTextLengthW(window: *mut c_void) -> i32;
    fn GetWindowTextW(window: *mut c_void, buffer: *mut u16, max_count: i32) -> i32;
    fn GetClassNameW(window: *mut c_void, buffer: *mut u16, max_count: i32) -> i32;
    fn ShowWindowAsync(window: *mut c_void, command: i32) -> i32;
}

#[repr(C)]
#[derive(Clone)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; MAX_PATH_WIDE],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ProcessMemoryCountersEx {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

fn collect_process_snapshot_entries() -> Result<Vec<ProcessSnapshotEntry>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot as isize == -1 {
        return Err(String::from("failed to capture Windows process snapshot"));
    }

    let mut entries = Vec::new();
    let mut process_entry = ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; MAX_PATH_WIDE],
    };

    let first_ok = unsafe { Process32FirstW(snapshot, &mut process_entry) };
    if first_ok != 0 {
        loop {
            entries.push(ProcessSnapshotEntry {
                process_id: process_entry.th32_process_id,
                parent_process_id: process_entry.th32_parent_process_id,
                process_name: utf16_buffer_to_string(&process_entry.sz_exe_file),
                thread_count: process_entry.cnt_threads,
            });

            let next_ok = unsafe { Process32NextW(snapshot, &mut process_entry) };
            if next_ok == 0 {
                break;
            }
        }
    }

    unsafe {
        CloseHandle(snapshot);
    }

    Ok(entries)
}

fn enumerate_visible_window_surfaces(
    processes: &HashMap<u32, TargetProcessRecord>,
) -> Result<Vec<RuntimeWindowSurface>, String> {
    let mut windows = enumerate_observed_window_surfaces(processes)?
        .into_iter()
        .map(|window| window.surface)
        .collect::<Vec<_>>();
    sort_window_surfaces(&mut windows);
    Ok(windows)
}

fn enumerate_observed_window_surfaces(
    processes: &HashMap<u32, TargetProcessRecord>,
) -> Result<Vec<ObservedWindowSurface>, String> {
    let mut context = WindowEnumerationContext {
        processes,
        windows: Vec::new(),
    };

    let ok = unsafe {
        EnumWindows(
            Some(enum_visible_window_surface),
            (&mut context as *mut WindowEnumerationContext<'_>) as isize,
        )
    };
    if ok == 0 {
        return Err(String::from("failed to enumerate visible Windows surfaces"));
    }

    Ok(context.windows)
}

unsafe extern "system" fn enum_visible_window_surface(window: *mut c_void, lparam: isize) -> i32 {
    let context = unsafe { &mut *(lparam as *mut WindowEnumerationContext<'_>) };
    if unsafe { IsWindowVisible(window) } == 0 {
        return 1;
    }

    let mut process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    let Some(process) = context.processes.get(&process_id) else {
        return 1;
    };
    if !process.is_running() {
        return 1;
    }
    let title = read_window_text(window);
    let class_name = read_window_class_name(window);
    if is_non_rendering_pseudo_console_surface(
        window_has_rendered_area(window),
        &title,
        &class_name,
    ) {
        return 1;
    }
    if title.is_empty() && class_name.is_empty() {
        return 1;
    }

    context.windows.push(ObservedWindowSurface {
        window_handle: window as usize,
        surface: RuntimeWindowSurface {
            process_key: process.process_key.clone(),
            display_name: process.display_name.clone(),
            relation: String::from(process.relation),
            pid: process.pid,
            process_name: process.process_name.clone(),
            window_handle: format!("0x{:X}", window as usize),
            title,
            class_name,
        },
    });

    1
}

fn window_has_rendered_area(window: *mut c_void) -> Option<bool> {
    let mut rect = WindowRect::default();
    if unsafe { GetWindowRect(window, &mut rect) } == 0 {
        return None;
    }
    Some(window_rect_has_positive_area(rect))
}

fn window_rect_has_positive_area(rect: WindowRect) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

fn is_non_rendering_pseudo_console_surface(
    has_rendered_area: Option<bool>,
    title: &str,
    class_name: &str,
) -> bool {
    has_rendered_area == Some(false)
        && title.is_empty()
        && class_name.eq_ignore_ascii_case("PseudoConsoleWindow")
}

fn hide_window_surface(window_handle: usize, expected_pid: u32) -> bool {
    let window = window_handle as *mut c_void;
    const SW_HIDE: i32 = 0;
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid != expected_pid {
        return false;
    }
    // ShowWindow can wait on the game's UI thread indefinitely. Queue the
    // request and confirm it through subsequent enumeration instead.
    unsafe { ShowWindowAsync(window, SW_HIDE) != 0 }
}

fn confirmed_suppression_count(
    requested: &HashSet<usize>,
    remaining: &[ObservedWindowSurface],
) -> usize {
    requested
        .iter()
        .filter(|handle| {
            !remaining
                .iter()
                .any(|window| window.window_handle == **handle)
        })
        .count()
}

fn read_window_text(window: *mut c_void) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0_u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if copied <= 0 {
        return String::new();
    }

    utf16_buffer_to_string(&buffer)
}

fn read_window_class_name(window: *mut c_void) -> String {
    let mut buffer = vec![0_u16; 256];
    let copied = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if copied <= 0 {
        return String::new();
    }

    utf16_buffer_to_string(&buffer)
}

fn sort_window_surfaces(windows: &mut [RuntimeWindowSurface]) {
    windows.sort_by(|left, right| {
        window_relation_priority(&left.relation)
            .cmp(&window_relation_priority(&right.relation))
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.process_name.cmp(&right.process_name))
            .then_with(|| left.pid.cmp(&right.pid))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.window_handle.cmp(&right.window_handle))
    });
}

fn window_relation_priority(relation: &str) -> u8 {
    match relation {
        "tracked_process" => 0,
        "descendant_process" => 1,
        _ => 9,
    }
}

fn read_cpu_profile() -> CpuProfile {
    if let Some(record) = read_processor_profile_record() {
        let name = record
            .name
            .map(|value| normalize_whitespace(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                read_registry_value(
                    r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
                    "ProcessorNameString",
                )
                .map(|value| normalize_whitespace(&value))
                .unwrap_or_default()
            });
        let logical_cores = record
            .number_of_logical_processors
            .or_else(|| {
                std::thread::available_parallelism()
                    .map(|parallelism| parallelism.get() as u32)
                    .ok()
            })
            .unwrap_or(0);

        return CpuProfile {
            name,
            frequency_mhz: record.current_clock_speed.unwrap_or(0),
            max_frequency_mhz: record.max_clock_speed.unwrap_or(0),
            physical_cores: record.number_of_cores.unwrap_or(0),
            logical_cores,
        };
    }

    let key = r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0";
    let name = read_registry_value(key, "ProcessorNameString")
        .map(|value| normalize_whitespace(&value))
        .unwrap_or_default();
    let frequency_mhz = read_registry_value(key, "~MHz")
        .and_then(|value| parse_registry_u32(&value))
        .unwrap_or(0);
    let logical_cores = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get() as u32)
        .unwrap_or(0);

    CpuProfile {
        name,
        frequency_mhz,
        max_frequency_mhz: frequency_mhz,
        physical_cores: 0,
        logical_cores,
    }
}

fn read_processor_profile_record() -> Option<ProcessorProfileRecord> {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Get-CimInstance Win32_Processor |
  Select-Object -First 1 Name, MaxClockSpeed, CurrentClockSpeed, NumberOfCores, NumberOfLogicalProcessors |
  ConvertTo-Json -Compress
"#;

    let output = powershell_json(script)?;
    deserialize_powershell_json_object(&output)
}

fn read_cpu_core_metrics() -> (Vec<CpuCoreMetrics>, TelemetryStatus) {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$rows = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation |
  Where-Object { $_.Name -notmatch '_Total$' -and $_.Name -notlike '*/*' } |
  ForEach-Object {
    [pscustomobject]@{
      Name = $_.Name
      PercentProcessorUtility = $_.PercentProcessorUtility
      PercentProcessorPerformance = $_.PercentProcessorPerformance
      ProcessorFrequency = $_.ProcessorFrequency
    }
  }
@($rows) | ConvertTo-Json -Compress
"#;

    let Some(output) = powershell_json(script) else {
        return (Vec::new(), TelemetryStatus::Unavailable);
    };

    let records =
        deserialize_powershell_json_array::<ProcessorCounterRecord>(&output).unwrap_or_default();
    let status = if !records.is_empty()
        && records.iter().all(|record| {
            record
                .percent_processor_utility
                .is_some_and(|value| value.is_finite() && value >= 0.0)
        }) {
        TelemetryStatus::Valid
    } else {
        TelemetryStatus::Unavailable
    };
    let cores = records
        .into_iter()
        .map(|record| CpuCoreMetrics {
            name: normalize_whitespace(record.name.as_deref().unwrap_or_default()),
            utility_percent: record
                .percent_processor_utility
                .unwrap_or(0.0)
                .clamp(0.0, 100.0),
            performance_percent: record.percent_processor_performance.unwrap_or(0.0),
            frequency_mhz: record.processor_frequency.unwrap_or(0),
        })
        .collect();
    (cores, status)
}

fn read_registry_value(key: &str, value_name: &str) -> Option<String> {
    let output = system_utility::capture(
        SystemUtility::Registry,
        &["query", key, "/v", value_name],
        QUERY_TIMEOUT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }

    parse_registry_query_output(&String::from_utf8_lossy(&output.stdout), value_name)
}

fn parse_registry_query_output(stdout: &str, value_name: &str) -> Option<String> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(value_name) {
            continue;
        }

        let parts = trimmed.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 3 {
            return Some(parts[2..].join(" "));
        }
    }

    None
}

fn parse_registry_u32(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u32>().ok()
    }
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read_cpu_times() -> Option<CpuTimes> {
    let mut idle = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();

    let result = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if result == 0 {
        return None;
    }

    let idle = file_time_to_u64(idle);
    let kernel = file_time_to_u64(kernel);
    let user = file_time_to_u64(user);

    Some(CpuTimes {
        idle,
        total: kernel.saturating_add(user),
    })
}

fn read_memory_snapshot() -> MemorySnapshot {
    let mut status = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };

    let result = unsafe { GlobalMemoryStatusEx(&mut status) };
    if result == 0 {
        return MemorySnapshot::default();
    }

    MemorySnapshot {
        load_percent: status.dw_memory_load as f32,
        total_bytes: status.ull_total_phys,
        available_bytes: status.ull_avail_phys,
    }
}

fn read_memory_modules() -> Vec<MemoryModuleMetrics> {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$rows = Get-CimInstance Win32_PhysicalMemory | ForEach-Object {
  [pscustomobject]@{
    BankLabel = $_.BankLabel
    DeviceLocator = $_.DeviceLocator
    Manufacturer = $_.Manufacturer
    PartNumber = $_.PartNumber
    Capacity = $_.Capacity
    Speed = $_.Speed
    ConfiguredClockSpeed = $_.ConfiguredClockSpeed
    ConfiguredVoltage = $_.ConfiguredVoltage
    SMBIOSMemoryType = $_.SMBIOSMemoryType
  }
}
@($rows) | ConvertTo-Json -Compress
"#;

    let Some(output) = powershell_json(script) else {
        return Vec::new();
    };

    deserialize_powershell_json_array::<PhysicalMemoryRecord>(&output)
        .unwrap_or_default()
        .into_iter()
        .map(memory_module_from_record)
        .collect()
}

pub fn read_process_memory_metrics(pids: &[u32]) -> Vec<ProcessMemoryMetrics> {
    let mut unique_pids = pids
        .iter()
        .copied()
        .filter(|pid| *pid > 0)
        .collect::<Vec<_>>();
    unique_pids.sort_unstable();
    unique_pids.dedup();

    if unique_pids.is_empty() {
        return Vec::new();
    }

    let Ok(snapshot) = collect_process_snapshot_entries() else {
        return Vec::new();
    };
    let mut by_pid = HashMap::with_capacity(snapshot.len());
    let mut children_by_parent = HashMap::<u32, Vec<&ProcessSnapshotEntry>>::new();
    for entry in &snapshot {
        by_pid.insert(entry.process_id, entry);
        children_by_parent
            .entry(entry.parent_process_id)
            .or_default()
            .push(entry);
    }

    let mut metrics = Vec::new();
    for target_pid in unique_pids {
        let mut queue = VecDeque::from([target_pid]);
        let mut seen = HashSet::new();

        while let Some(process_id) = queue.pop_front() {
            if !seen.insert(process_id) {
                continue;
            }

            if let Some(entry) = by_pid.get(&process_id) {
                let counters = read_process_counters(entry.process_id);
                metrics.push(ProcessMemoryMetrics {
                    target_process_id: target_pid,
                    process_id: entry.process_id,
                    parent_process_id: entry.parent_process_id,
                    name: normalize_whitespace(&entry.process_name),
                    working_set_bytes: counters.working_set_bytes,
                    page_file_usage_bytes: counters.page_file_usage_bytes,
                    thread_count: entry.thread_count,
                    handle_count: counters.handle_count,
                });
            }

            if let Some(children) = children_by_parent.get(&process_id) {
                for child in children {
                    queue.push_back(child.process_id);
                }
            }
        }
    }

    metrics.sort_by(|left, right| {
        left.target_process_id
            .cmp(&right.target_process_id)
            .then_with(|| left.process_id.cmp(&right.process_id))
    });
    metrics
}

#[derive(Debug, Clone, Copy, Default)]
struct NativeProcessCounters {
    working_set_bytes: u64,
    page_file_usage_bytes: u64,
    handle_count: u32,
}

fn read_process_counters(pid: u32) -> NativeProcessCounters {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if handle.is_null() {
        return NativeProcessCounters::default();
    }

    let mut counters = ProcessMemoryCountersEx {
        cb: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
        private_usage: 0,
    };
    let memory_ok = unsafe {
        GetProcessMemoryInfo(
            handle,
            &mut counters,
            std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
        )
    } != 0;

    let mut handle_count = 0_u32;
    let handle_count_ok = unsafe { GetProcessHandleCount(handle, &mut handle_count) } != 0;
    unsafe {
        CloseHandle(handle);
    }

    NativeProcessCounters {
        working_set_bytes: if memory_ok {
            counters.working_set_size as u64
        } else {
            0
        },
        page_file_usage_bytes: if memory_ok {
            counters.pagefile_usage as u64
        } else {
            0
        },
        handle_count: if handle_count_ok { handle_count } else { 0 },
    }
}

fn memory_module_from_record(record: PhysicalMemoryRecord) -> MemoryModuleMetrics {
    let part_number = normalize_whitespace(record.part_number.as_deref().unwrap_or_default());
    let speed_mts = record.speed.unwrap_or(0);
    let configured_clock_mts = record.configured_clock_speed.unwrap_or(0);
    let inferred_cas_latency = infer_cas_latency_from_part_number(&part_number);
    let memory_type = memory_type_label(record.smbios_memory_type.unwrap_or(0)).to_string();
    let timing_summary = match inferred_cas_latency {
        Some(cas) if configured_clock_mts > 0 => {
            format!("{}-{} CL{}", memory_type, configured_clock_mts, cas)
        }
        Some(cas) => format!("{} CL{}", memory_type, cas),
        None if configured_clock_mts > 0 => format!("{}-{}", memory_type, configured_clock_mts),
        None => memory_type.clone(),
    };

    MemoryModuleMetrics {
        bank_label: normalize_whitespace(record.bank_label.as_deref().unwrap_or_default()),
        device_locator: normalize_whitespace(record.device_locator.as_deref().unwrap_or_default()),
        manufacturer: normalize_whitespace(record.manufacturer.as_deref().unwrap_or_default()),
        part_number,
        capacity_bytes: record.capacity.unwrap_or(0),
        speed_mts,
        configured_clock_mts,
        configured_voltage_mv: record.configured_voltage.unwrap_or(0),
        memory_type,
        inferred_cas_latency,
        timing_summary,
    }
}

fn infer_cas_latency_from_part_number(part_number: &str) -> Option<u32> {
    let bytes = part_number.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index].eq_ignore_ascii_case(&b'C') {
            let digits = bytes
                .iter()
                .skip(index + 1)
                .take_while(|byte| byte.is_ascii_digit())
                .map(|byte| (byte - b'0') as u32)
                .collect::<Vec<_>>();
            if digits.len() >= 2 {
                return Some(digits.iter().fold(0, |acc, digit| acc * 10 + digit));
            }
        }
    }

    None
}

fn memory_type_label(memory_type: u32) -> &'static str {
    match memory_type {
        20 => "DDR",
        21 => "DDR2",
        24 => "DDR3",
        26 => "DDR4",
        34 => "DDR5",
        _ => "RAM",
    }
}

fn read_disk_snapshot(volume_id: &str, mount: &str) -> DiskSnapshot {
    let disk_path = Path::new(mount);
    let label = host_inventory::volume_display_label(volume_id, mount);
    let disk_performance = read_disk_performance_snapshot(&label);
    let wide_path = wide_null(disk_path.as_os_str());

    let mut free_bytes_available = 0_u64;
    let mut total_bytes = 0_u64;
    let mut total_free_bytes = 0_u64;
    let result = unsafe {
        GetDiskFreeSpaceExW(
            wide_path.as_ptr(),
            &mut free_bytes_available,
            &mut total_bytes,
            &mut total_free_bytes,
        )
    };

    if result == 0 || total_bytes == 0 {
        return DiskSnapshot {
            io_status: disk_performance.5,
            label,
            read_bps: disk_performance.0,
            write_bps: disk_performance.1,
            read_latency_ms: disk_performance.2,
            write_latency_ms: disk_performance.3,
            queue_length: disk_performance.4,
            ..DiskSnapshot::default()
        };
    }

    let used_bytes = total_bytes.saturating_sub(total_free_bytes);
    let (volume_name, file_system) = read_volume_identity(disk_path);

    DiskSnapshot {
        io_status: disk_performance.5,
        used_percent: (used_bytes as f64 / total_bytes as f64 * 100.0) as f32,
        used_bytes,
        total_bytes,
        label,
        volume_name,
        file_system,
        read_bps: disk_performance.0,
        write_bps: disk_performance.1,
        read_latency_ms: disk_performance.2,
        write_latency_ms: disk_performance.3,
        queue_length: disk_performance.4,
    }
}

fn read_disk_performance_snapshot(label: &str) -> (u64, u64, f32, f32, f32, TelemetryStatus) {
    let normalized_label = label.trim_end_matches('\\').to_ascii_lowercase();
    let counters = read_disk_counter_records();
    let counter = counters.iter().find(|record| {
        record
            .name
            .as_deref()
            .map(|name| name.eq_ignore_ascii_case(&normalized_label))
            .unwrap_or(false)
    });

    let read_latency = read_logical_disk_latency_ms(&normalized_label, "Read");
    let write_latency = read_logical_disk_latency_ms(&normalized_label, "Write");

    (
        counter
            .and_then(|record| record.disk_read_bytes_persec)
            .unwrap_or(0),
        counter
            .and_then(|record| record.disk_write_bytes_persec)
            .unwrap_or(0),
        read_latency.unwrap_or(0.0),
        write_latency.unwrap_or(0.0),
        counter
            .and_then(|record| record.current_disk_queue_length)
            .unwrap_or(0.0),
        if read_latency.is_some()
            && write_latency.is_some()
            && counter.is_some_and(|record| {
                record.disk_read_bytes_persec.is_some()
                    && record.disk_write_bytes_persec.is_some()
                    && record
                        .current_disk_queue_length
                        .is_some_and(|value| value.is_finite() && value >= 0.0)
            })
        {
            TelemetryStatus::Valid
        } else {
            TelemetryStatus::Unavailable
        },
    )
}

fn read_disk_counter_records() -> Vec<DiskCounterRecord> {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$rows = Get-CimInstance Win32_PerfFormattedData_PerfDisk_LogicalDisk |
  ForEach-Object {
    [pscustomobject]@{
      Name = $_.Name
      DiskReadBytesPersec = $_.DiskReadBytesPersec
      DiskWriteBytesPersec = $_.DiskWriteBytesPersec
      CurrentDiskQueueLength = $_.CurrentDiskQueueLength
    }
  }
@($rows) | ConvertTo-Json -Compress
"#;

    let Some(output) = powershell_json(script) else {
        return Vec::new();
    };

    deserialize_powershell_json_array::<DiskCounterRecord>(&output).unwrap_or_default()
}

fn read_logical_disk_latency_ms(label: &str, direction: &str) -> Option<f32> {
    let counter_name = match direction {
        "Read" => "Avg. Disk sec/Read",
        "Write" => "Avg. Disk sec/Write",
        _ => return None,
    };
    let escaped_label = label.replace('\'', "''");
    let escaped_counter = counter_name.replace('\'', "''");
    let script = format!(
        r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$path = "\LogicalDisk({})\{}"
$sample = Get-Counter $path -ErrorAction SilentlyContinue
if ($sample -and $sample.CounterSamples) {{
  [pscustomobject]@{{ Value = $sample.CounterSamples[0].CookedValue; Status = $sample.CounterSamples[0].Status }} | ConvertTo-Json -Compress
}}
"#,
        escaped_label, escaped_counter
    );

    let output = powershell_json(&script)?;

    #[derive(Deserialize)]
    struct CounterValue {
        #[serde(rename = "Value")]
        value: Option<f64>,
        #[serde(rename = "Status")]
        status: Option<u32>,
    }

    deserialize_powershell_json_object::<CounterValue>(&output)
        .and_then(|counter| host_telemetry::disk_latency_ms(counter.value, counter.status))
}

fn read_volume_identity(root_path: &Path) -> (String, String) {
    let wide_path = wide_null(root_path.as_os_str());
    let mut volume_name = [0_u16; 260];
    let mut file_system = [0_u16; 260];
    let mut serial_number = 0_u32;
    let mut max_component_length = 0_u32;
    let mut file_system_flags = 0_u32;

    let result = unsafe {
        GetVolumeInformationW(
            wide_path.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            &mut serial_number,
            &mut max_component_length,
            &mut file_system_flags,
            file_system.as_mut_ptr(),
            file_system.len() as u32,
        )
    };

    if result == 0 {
        return (String::new(), String::new());
    }

    (
        utf16_buffer_to_string(&volume_name),
        utf16_buffer_to_string(&file_system),
    )
}

fn read_network_totals() -> Option<(u64, u64)> {
    let output = system_utility::capture(SystemUtility::Network, &["-e"], QUERY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }

    parse_network_totals(&host_telemetry::decode_network_counter_output(
        &output.stdout,
    )?)
}

fn inspect_process_network_endpoints(
    targets: &[WindowInspectionTarget],
    ports: &[u16],
) -> Result<ProcessNetworkInspectionResult, String> {
    let processes = collect_verified_target_process_records(targets)?;
    if processes.is_empty() {
        return Ok(ProcessNetworkInspectionResult {
            inspected_process_count: 0,
            endpoints: Vec::new(),
        });
    }

    let output =
        system_utility::capture(SystemUtility::Network, &["-a", "-n", "-o"], QUERY_TIMEOUT)
            .map_err(|error| format!("failed to inspect Windows network endpoints: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows network endpoint inspection failed with status {}",
            output.status
        ));
    }

    let requested_ports = ports.iter().copied().collect::<HashSet<_>>();
    let mut endpoints = parse_netstat_network_endpoints(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .filter(|endpoint| {
            processes.contains_key(&endpoint.owning_pid)
                && (requested_ports.is_empty() || requested_ports.contains(&endpoint.local_port))
        })
        .filter_map(|endpoint| {
            let process = processes.get(&endpoint.owning_pid)?;
            Some(ProcessNetworkEndpoint {
                protocol: endpoint.protocol,
                local_address: endpoint.local_address,
                local_port: endpoint.local_port,
                owning_pid: endpoint.owning_pid,
                process_key: process.process_key.clone(),
                relation: String::from(process.relation),
            })
        })
        .collect::<Vec<_>>();
    endpoints.sort_by(|left, right| {
        left.local_port
            .cmp(&right.local_port)
            .then_with(|| left.protocol.cmp(&right.protocol))
            .then_with(|| left.local_address.cmp(&right.local_address))
            .then_with(|| left.owning_pid.cmp(&right.owning_pid))
    });
    endpoints.dedup();

    Ok(ProcessNetworkInspectionResult {
        inspected_process_count: processes.len(),
        endpoints,
    })
}

fn parse_netstat_network_endpoints(stdout: &str) -> Vec<ParsedNetworkEndpoint> {
    stdout
        .lines()
        .filter_map(|line| {
            let columns = line.split_whitespace().collect::<Vec<_>>();
            let protocol = columns.first()?.to_ascii_lowercase();
            if protocol != "tcp" && protocol != "udp" {
                return None;
            }
            if protocol == "tcp"
                && !columns
                    .get(3)
                    .is_some_and(|state| state.eq_ignore_ascii_case("LISTENING"))
            {
                return None;
            }
            let (local_address, local_port) = parse_netstat_local_endpoint(columns.get(1)?)?;
            let owning_pid = columns.last()?.parse::<u32>().ok()?;
            Some(ParsedNetworkEndpoint {
                protocol,
                local_address,
                local_port,
                owning_pid,
            })
        })
        .collect()
}

fn parse_netstat_local_endpoint(value: &str) -> Option<(String, u16)> {
    let (address, port) = value.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    let address = address.trim_matches(['[', ']']);
    let address_without_scope = address.split_once('%').map_or(address, |(ip, _)| ip);
    let normalized = address_without_scope
        .parse::<IpAddr>()
        .map(|ip| match ip {
            IpAddr::V6(ipv6) => ipv6
                .to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ipv6)),
            other => other,
        })
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| address_without_scope.to_string());
    Some((normalized, port))
}

fn read_network_adapter_records() -> Vec<NetworkAdapterRecord> {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$stats = @{}
Get-NetAdapterStatistics -ErrorAction SilentlyContinue | ForEach-Object { $stats[$_.Name] = $_ }
$ips = @{}
Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | ForEach-Object {
  if (-not $ips.ContainsKey($_.InterfaceAlias)) { $ips[$_.InterfaceAlias] = @() }
  $ips[$_.InterfaceAlias] += $_.IPAddress
}
$rows = Get-CimInstance Win32_NetworkAdapter |
  Where-Object { $_.NetConnectionID } |
  ForEach-Object {
    $name = $_.NetConnectionID
    $adapterStats = $stats[$name]
    [pscustomobject]@{
      Name = $name
      Description = $_.Description
      Status = if ($_.NetEnabled) { 'Up' } else { 'Down' }
      LinkSpeedBps = if ($_.Speed) { [UInt64]$_.Speed } else { 0 }
      MacAddress = $_.MACAddress
      IPv4Addresses = @($ips[$name])
      ReceivedBytes = if ($adapterStats) { [UInt64]$adapterStats.ReceivedBytes } else { $null }
      SentBytes = if ($adapterStats) { [UInt64]$adapterStats.SentBytes } else { $null }
    }
  }
@($rows) | ConvertTo-Json -Compress -Depth 4
"#;

    let Some(output) = powershell_json(script) else {
        return Vec::new();
    };

    deserialize_powershell_json_array::<NetworkAdapterRecord>(&output).unwrap_or_default()
}

fn parse_network_totals(stdout: &str) -> Option<(u64, u64)> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        if let Some(label) = parts.next()
            && (label.eq_ignore_ascii_case("bytes") || label == "字节")
        {
            let received = parse_u64_token(parts.next()?)?;
            let transmitted = parse_u64_token(parts.next()?)?;
            return parts.next().is_none().then_some((received, transmitted));
        }
    }

    None
}

fn parse_u64_token(token: &str) -> Option<u64> {
    if token.is_empty()
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b',')
    {
        return None;
    }
    if token.contains(',') {
        let mut groups = token.split(',');
        if !(1..=3).contains(&groups.next()?.len()) || groups.any(|group| group.len() != 3) {
            return None;
        }
    }
    token.replace(',', "").parse::<u64>().ok()
}

fn read_bind_address_candidates() -> Vec<BindAddressCandidate> {
    let records = read_net_ip_configuration_records();
    build_bind_address_candidates(records)
}

fn read_net_ip_configuration_records() -> Vec<NetIpConfigurationRecord> {
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$rows = Get-NetIPConfiguration |
  Where-Object { $_.NetAdapter.Status -eq 'Up' -and $_.IPv4Address } |
  ForEach-Object {
    $alias = $_.InterfaceAlias
    $description = $_.NetAdapter.InterfaceDescription
    foreach ($address in @($_.IPv4Address)) {
      [pscustomobject]@{
        IPAddress = $address.IPAddress
        InterfaceAlias = $alias
        InterfaceDescription = $description
      }
    }
  }
@($rows) | ConvertTo-Json -Compress
"#;

    let Some(output) = powershell_json(script) else {
        return Vec::new();
    };

    deserialize_net_ip_configuration_records(&output).unwrap_or_default()
}

fn deserialize_net_ip_configuration_records(
    stdout: &[u8],
) -> Option<Vec<NetIpConfigurationRecord>> {
    deserialize_powershell_json_array(stdout)
}

fn deserialize_powershell_json_array<T: DeserializeOwned>(stdout: &[u8]) -> Option<Vec<T>> {
    let payload = decode_powershell_text(stdout)
        .trim_matches('\u{feff}')
        .trim()
        .to_string();
    if payload.is_empty() {
        return Some(Vec::new());
    }

    let value: Value = serde_json::from_str(&payload).ok()?;
    match value {
        Value::Array(items) => serde_json::from_value(Value::Array(items)).ok(),
        Value::Object(_) => serde_json::from_value(Value::Array(vec![value])).ok(),
        _ => Some(Vec::new()),
    }
}

fn deserialize_powershell_json_object<T: DeserializeOwned>(stdout: &[u8]) -> Option<T> {
    let payload = decode_powershell_text(stdout)
        .trim_matches('\u{feff}')
        .trim()
        .to_string();
    if payload.is_empty() {
        return None;
    }

    serde_json::from_str(&payload).ok()
}

fn powershell_json(script: &str) -> Option<Vec<u8>> {
    let output = system_utility::capture(
        SystemUtility::PowerShell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ],
        QUERY_TIMEOUT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(output.stdout)
}

fn decode_powershell_text(stdout: &[u8]) -> String {
    decode_utf16_with_bom(stdout)
        .or_else(|| String::from_utf8(stdout.to_vec()).ok())
        .or_else(|| decode_utf16_without_bom(stdout))
        .unwrap_or_else(|| String::from_utf8_lossy(stdout).into_owned())
}

fn decode_utf16_with_bom(stdout: &[u8]) -> Option<String> {
    if stdout.len() < 2 {
        return None;
    }

    if stdout.starts_with(&[0xFF, 0xFE]) {
        return Some(decode_utf16_bytes(&stdout[2..], true));
    }

    if stdout.starts_with(&[0xFE, 0xFF]) {
        return Some(decode_utf16_bytes(&stdout[2..], false));
    }

    None
}

fn decode_utf16_without_bom(stdout: &[u8]) -> Option<String> {
    if stdout.len() < 2 || !stdout.len().is_multiple_of(2) {
        return None;
    }

    let even_nulls = stdout.iter().step_by(2).filter(|byte| **byte == 0).count();
    let odd_nulls = stdout
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    if even_nulls == 0 && odd_nulls == 0 {
        return None;
    }

    Some(decode_utf16_bytes(stdout, odd_nulls >= even_nulls))
}

fn decode_utf16_bytes(stdout: &[u8], little_endian: bool) -> String {
    let mut code_units = Vec::with_capacity(stdout.len() / 2);
    for chunk in stdout.as_chunks::<2>().0 {
        let code_unit = if little_endian {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], chunk[1]])
        };
        code_units.push(code_unit);
    }

    String::from_utf16_lossy(&code_units)
}

fn build_bind_address_candidates(
    records: Vec<NetIpConfigurationRecord>,
) -> Vec<BindAddressCandidate> {
    let families = WindowsPlatform::supported_overlay_families();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for record in records {
        let address_text = record.ip_address.trim();
        let Ok(address) = address_text.parse::<Ipv4Addr>() else {
            continue;
        };

        if address.is_unspecified() || address.is_loopback() || is_link_local_v4(address) {
            continue;
        }

        let overlay_family = overlay_family_for_adapter(
            record.interface_alias.as_deref(),
            record.interface_description.as_deref(),
            &families,
        );

        if overlay_family.is_none()
            && is_noise_adapter(
                record.interface_alias.as_deref(),
                record.interface_description.as_deref(),
            )
        {
            continue;
        }

        if !seen.insert(address.to_string()) {
            continue;
        }

        candidates.push(BindAddressCandidate {
            address: address.to_string(),
            kind: classify_bind_address(address, overlay_family.is_some()).to_string(),
            adapter_name: format_adapter_name(
                record.interface_alias.as_deref(),
                record.interface_description.as_deref(),
            ),
            family_name: overlay_family.map(|family| family.name.clone()),
        });
    }

    candidates.sort_by(|left, right| {
        bind_kind_priority(&left.kind)
            .cmp(&bind_kind_priority(&right.kind))
            .then_with(|| left.family_name.cmp(&right.family_name))
            .then_with(|| left.adapter_name.cmp(&right.adapter_name))
            .then_with(|| left.address.cmp(&right.address))
    });

    candidates
}

fn classify_bind_address(address: Ipv4Addr, overlay: bool) -> &'static str {
    if overlay {
        "overlay"
    } else if is_benchmarking_or_proxy_v4(address) {
        "proxy"
    } else if address.is_private() || is_carrier_grade_nat(address) {
        "lan"
    } else {
        "public"
    }
}

fn bind_kind_priority(kind: &str) -> u8 {
    match kind {
        "overlay" => 1,
        "lan" => 2,
        "public" => 3,
        "proxy" => 8,
        _ => 9,
    }
}

fn network_adapter_priority(adapter: &NetworkAdapterMetrics) -> u8 {
    if adapter.family_name.is_some() {
        0
    } else if adapter.status.eq_ignore_ascii_case("up") {
        1
    } else if is_noise_adapter(Some(&adapter.name), Some(&adapter.description)) {
        8
    } else {
        4
    }
}

fn overlay_family_for_adapter<'a>(
    alias: Option<&str>,
    description: Option<&str>,
    families: &'a [OverlayFamily],
) -> Option<&'a OverlayFamily> {
    families
        .iter()
        .find(|family| matches_overlay_family(family, alias, description))
}

fn matches_overlay_family(
    family: &OverlayFamily,
    alias: Option<&str>,
    description: Option<&str>,
) -> bool {
    family.adapter_name_patterns.iter().any(|pattern| {
        contains_ignore_ascii_case(alias, pattern)
            || contains_ignore_ascii_case(description, pattern)
    })
}

fn contains_ignore_ascii_case(value: Option<&str>, pattern: &str) -> bool {
    let Some(value) = value else {
        return false;
    };

    value
        .to_ascii_lowercase()
        .contains(&pattern.to_ascii_lowercase())
}

fn format_adapter_name(alias: Option<&str>, description: Option<&str>) -> Option<String> {
    let alias = alias.map(str::trim).filter(|value| !value.is_empty());
    let description = description.map(str::trim).filter(|value| !value.is_empty());

    alias
        .map(String::from)
        .or_else(|| description.map(String::from))
}

fn is_noise_adapter(alias: Option<&str>, description: Option<&str>) -> bool {
    const NOISE_PATTERNS: &[&str] = &[
        "loopback",
        "pseudo-interface",
        "teredo",
        "isatap",
        "vethernet",
        "hyper-v",
        "vmware",
        "virtualbox",
        "npcap",
        "wsl",
        "bluetooth",
    ];

    NOISE_PATTERNS.iter().any(|pattern| {
        contains_ignore_ascii_case(alias, pattern)
            || contains_ignore_ascii_case(description, pattern)
    })
}

fn is_link_local_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 169 && octets[1] == 254
}

fn is_carrier_grade_nat(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_benchmarking_or_proxy_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 198 && (18..=19).contains(&octets[1])
}

pub fn build_instance_firewall_rule_specs(
    instance_id: &str,
    instance_name: &str,
    ports: &[PortBinding],
    local_address: Option<&str>,
) -> Vec<WindowsFirewallRuleSpec> {
    let instance_segment = sanitize_firewall_rule_segment(instance_id)
        .or_else(|| sanitize_firewall_rule_segment(instance_name))
        .unwrap_or_else(|| String::from("instance"));
    let mut seen = HashSet::<(String, u16)>::new();
    let mut specs = Vec::new();
    let local_address = local_address.unwrap_or("Any").to_string();

    for port in ports {
        let Some(protocol) = normalize_firewall_protocol(&port.protocol) else {
            continue;
        };
        if !seen.insert((protocol.clone(), port.port)) {
            continue;
        }

        let port_segment =
            sanitize_firewall_rule_segment(&port.name).unwrap_or_else(|| String::from("port"));
        let rule_name = truncate_firewall_rule_name(format!(
            "LanGame {instance_segment} {port_segment} {protocol} {}",
            port.port
        ));
        specs.push(WindowsFirewallRuleSpec {
            rule_name,
            protocol,
            local_port: port.port,
            local_address: local_address.clone(),
        });
    }

    specs
}

fn ensure_instance_firewall_rules(
    instance_id: &str,
    instance_name: &str,
    ports: &[PortBinding],
    local_address: Option<&str>,
) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
    apply_firewall_rule_specs(&build_instance_firewall_rule_specs(
        instance_id,
        instance_name,
        ports,
        local_address,
    ))
}

fn apply_firewall_rule_specs(
    specs: &[WindowsFirewallRuleSpec],
) -> Result<Vec<WindowsFirewallRuleApplyResult>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }

    let script = build_firewall_rule_batch_script(specs)?;
    let output = system_utility::capture(
        SystemUtility::PowerShell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
        FIREWALL_TIMEOUT,
    )
    .map_err(|error| format!("failed to run Windows Firewall PowerShell command: {error}"))?;

    if !output.status.success() {
        let stderr = decode_powershell_text(&output.stderr).trim().to_string();
        let stdout = decode_powershell_text(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("powershell exited with status {}", output.status)
        };
        return Err(format!("failed to create Windows Firewall rules: {detail}"));
    }

    deserialize_powershell_json_object::<Vec<WindowsFirewallRuleApplyResult>>(&output.stdout)
        .ok_or_else(|| String::from("Windows Firewall command returned invalid JSON"))
}

fn build_firewall_rule_batch_script(specs: &[WindowsFirewallRuleSpec]) -> Result<String, String> {
    let rules_json = serde_json::to_string(specs)
        .map_err(|error| format!("failed to serialize Windows Firewall rules: {error}"))?;

    Ok(format!(
        r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$ErrorActionPreference = 'Stop'
$rulesJson = @'
{rules_json}
'@
$rules = $rulesJson | ConvertFrom-Json
$results = foreach ($rule in $rules) {{
  $name = [string]$rule.rule_name
  $protocol = [string]$rule.protocol
  $localPort = [int]$rule.local_port
  $localAddress = [string]$rule.local_address
  $existing = @(Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue | Where-Object {{ $_.DisplayName -eq $name }})
  $status = if ($existing.Count -gt 0) {{ 'replaced' }} else {{ 'created' }}
  if ($existing.Count -gt 0) {{
    $existing | Remove-NetFirewallRule -ErrorAction Stop
  }}
  New-NetFirewallRule -DisplayName $name -Direction Inbound -Action Allow -Protocol $protocol -LocalPort $localPort -LocalAddress $localAddress -Profile Any -Enabled True -Group 'LanGame Server Manager' -Description 'Created by LanGame Server Manager for a managed server instance port.' | Out-Null
  [pscustomobject]@{{
    rule_name = $name
    protocol = $protocol
    local_port = $localPort
    local_address = $localAddress
    status = $status
    message = $null
  }}
}}
ConvertTo-Json -InputObject @($results) -Compress
"#,
        rules_json = rules_json,
    ))
}

fn normalize_firewall_protocol(protocol: &str) -> Option<String> {
    if protocol.eq_ignore_ascii_case("tcp") {
        Some(String::from("TCP"))
    } else if protocol.eq_ignore_ascii_case("udp") {
        Some(String::from("UDP"))
    } else {
        None
    }
}

fn sanitize_firewall_rule_segment(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut last_was_dash = false;

    for ch in value.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.') {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        output.push(mapped);
    }

    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        None
    } else if trimmed.len() > 48 {
        Some(trimmed.chars().take(48).collect())
    } else {
        Some(trimmed)
    }
}

fn truncate_firewall_rule_name(rule_name: String) -> String {
    if rule_name.len() <= 150 {
        rule_name
    } else {
        rule_name.chars().take(150).collect()
    }
}

fn file_time_to_u64(file_time: FileTime) -> u64 {
    ((file_time.dw_high_date_time as u64) << 32) | file_time.dw_low_date_time as u64
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn utf16_buffer_to_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let owned = OsString::from_wide(&buffer[..end]);
    owned.to_string_lossy().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_elevation_probe_returns_a_boolean() {
        assert!(matches!(
            WindowsPlatform::current_process_is_elevated(),
            Ok(true) | Ok(false)
        ));
    }

    #[test]
    fn netstat_endpoint_parser_normalizes_ipv4_mapped_ipv6() {
        let endpoints = parse_netstat_network_endpoints(
            "  UDP    [::ffff:192.168.31.150]:14159    *:*    33996\r\n",
        );

        assert_eq!(
            endpoints,
            vec![ParsedNetworkEndpoint {
                protocol: String::from("udp"),
                local_address: String::from("192.168.31.150"),
                local_port: 14159,
                owning_pid: 33996,
            }]
        );
    }

    #[test]
    fn netstat_endpoint_parser_keeps_wildcard_addresses_visible() {
        let endpoints = parse_netstat_network_endpoints(
            "  TCP    0.0.0.0:27015    0.0.0.0:0    LISTENING    1234\r\n  UDP    [::]:27016    *:*    1234\r\n",
        );

        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0].local_address, "0.0.0.0");
        assert_eq!(endpoints[1].local_address, "::");
    }

    #[test]
    fn window_surfaces_sort_tracked_processes_first() {
        let mut windows = vec![
            RuntimeWindowSurface {
                process_key: String::from("main"),
                display_name: String::from("Server"),
                relation: String::from("descendant_process"),
                pid: 101,
                process_name: String::from("conhost.exe"),
                window_handle: String::from("0x2"),
                title: String::from("Console"),
                class_name: String::from("ConsoleWindowClass"),
            },
            RuntimeWindowSurface {
                process_key: String::from("main"),
                display_name: String::from("Server"),
                relation: String::from("tracked_process"),
                pid: 100,
                process_name: String::from("server.exe"),
                window_handle: String::from("0x1"),
                title: String::from("Server UI"),
                class_name: String::from("GameWindowClass"),
            },
        ];

        sort_window_surfaces(&mut windows);

        assert_eq!(windows[0].relation, "tracked_process");
        assert_eq!(windows[1].relation, "descendant_process");
    }

    #[test]
    fn queued_window_suppression_is_not_reported_as_completed_while_visible() {
        let remaining = [ObservedWindowSurface {
            window_handle: 2,
            surface: RuntimeWindowSurface {
                process_key: String::from("main"),
                display_name: String::from("Server"),
                relation: String::from("tracked_process"),
                pid: 100,
                process_name: String::from("server.exe"),
                window_handle: String::from("0x2"),
                title: String::from("Server console"),
                class_name: String::from("ConsoleWindowClass"),
            },
        }];
        assert_eq!(
            confirmed_suppression_count(&HashSet::from([1, 2]), &remaining),
            1
        );
        assert_eq!(
            confirmed_suppression_count(&HashSet::from([2]), &remaining),
            0
        );
    }

    #[test]
    fn only_empty_non_rendering_pseudo_console_surfaces_are_ignored() {
        assert!(!window_rect_has_positive_area(WindowRect::default()));
        assert!(window_rect_has_positive_area(WindowRect {
            left: 10,
            top: 20,
            right: 640,
            bottom: 480,
        }));
        assert!(is_non_rendering_pseudo_console_surface(
            Some(false),
            "",
            "PseudoConsoleWindow"
        ));
        assert!(!is_non_rendering_pseudo_console_surface(
            Some(true),
            "",
            "PseudoConsoleWindow"
        ));
        assert!(!is_non_rendering_pseudo_console_surface(
            Some(false),
            "Server console",
            "PseudoConsoleWindow"
        ));
        assert!(!is_non_rendering_pseudo_console_surface(
            Some(false),
            "",
            "OtherWindowClass"
        ));
        assert!(!is_non_rendering_pseudo_console_surface(
            None,
            "",
            "PseudoConsoleWindow"
        ));
    }

    #[test]
    fn firewall_rule_specs_keep_unique_tcp_udp_ports() {
        let ports = vec![
            app_core::PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 8211,
            },
            app_core::PortBinding {
                name: String::from("duplicate"),
                protocol: String::from("UDP"),
                port: 8211,
            },
            app_core::PortBinding {
                name: String::from("query"),
                protocol: String::from("tcp"),
                port: 27015,
            },
            app_core::PortBinding {
                name: String::from("ignored"),
                protocol: String::from("http"),
                port: 8080,
            },
        ];

        let specs = build_instance_firewall_rule_specs(
            "palworld-main",
            "Palworld Main",
            &ports,
            Some("192.168.31.150"),
        );

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].rule_name, "LanGame palworld-main game UDP 8211");
        assert_eq!(specs[0].protocol, "UDP");
        assert_eq!(specs[0].local_port, 8211);
        assert_eq!(specs[0].local_address, "192.168.31.150");
        assert_eq!(specs[1].rule_name, "LanGame palworld-main query TCP 27015");
        assert_eq!(specs[1].protocol, "TCP");
        assert_eq!(specs[1].local_port, 27015);
    }

    #[test]
    fn firewall_rule_specs_keep_both_core_keeper_udp_endpoints() {
        let specs = build_instance_firewall_rule_specs(
            "corekeeper-main",
            "Core Keeper Main",
            &[
                app_core::PortBinding {
                    name: String::from("game"),
                    protocol: String::from("udp"),
                    port: 27_017,
                },
                app_core::PortBinding {
                    name: String::from("query"),
                    protocol: String::from("udp"),
                    port: 27_018,
                },
            ],
            None,
        );

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].local_port, 27_017);
        assert_eq!(specs[1].local_port, 27_018);
        assert!(specs.iter().all(|spec| spec.protocol == "UDP"));
    }

    #[test]
    fn firewall_rule_batch_script_contains_all_rule_specs_in_one_payload() {
        let specs = vec![
            WindowsFirewallRuleSpec {
                rule_name: String::from("LanGame alpha game UDP 8211"),
                protocol: String::from("UDP"),
                local_port: 8211,
                local_address: String::from("192.168.31.150"),
            },
            WindowsFirewallRuleSpec {
                rule_name: String::from("LanGame alpha query TCP 27015"),
                protocol: String::from("TCP"),
                local_port: 27015,
                local_address: String::from("192.168.31.150"),
            },
        ];

        let script = build_firewall_rule_batch_script(&specs).expect("batch script");

        assert!(script.contains("$rulesJson = @'"));
        assert!(script.contains("LanGame alpha game UDP 8211"));
        assert!(script.contains("LanGame alpha query TCP 27015"));
        assert!(script.contains("-LocalAddress $localAddress"));
        assert!(script.contains("$rules = $rulesJson | ConvertFrom-Json"));
        assert!(!script.contains("$rules = @($rulesJson | ConvertFrom-Json)"));
        assert!(script.contains("ConvertTo-Json -InputObject @($results) -Compress"));
        assert_eq!(script.matches("New-NetFirewallRule").count(), 1);
        assert_eq!(script.matches("ConvertFrom-Json").count(), 1);
    }

    #[test]
    #[cfg(windows)]
    fn firewall_rule_batch_script_roundtrips_multiple_rules_in_windows_powershell() {
        struct PhaseDirectory(std::path::PathBuf);
        impl Drop for PhaseDirectory {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).expect("remove firewall fixture phases");
            }
        }
        let phase_root = std::env::temp_dir().join(format!(
            "lgsm-firewall-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&phase_root).expect("create firewall fixture phases");
        let phases = PhaseDirectory(phase_root);
        let phase_path = phases.0.join("phases.log");
        let phase_path_literal = phase_path.to_string_lossy().replace('\'', "''");
        let specs = vec![
            WindowsFirewallRuleSpec {
                rule_name: String::from("LanGame alpha game UDP 8211"),
                protocol: String::from("UDP"),
                local_port: 8211,
                local_address: String::from("192.168.31.150"),
            },
            WindowsFirewallRuleSpec {
                rule_name: String::from("LanGame alpha query TCP 27015"),
                protocol: String::from("TCP"),
                local_port: 27015,
                local_address: String::from("192.168.31.150"),
            },
        ];
        let firewall_script = build_firewall_rule_batch_script(&specs).expect("batch script");
        let script = format!(
            r#"
$phasePath = '{phase_path_literal}'
function Write-TestPhase {{
  param([string]$Phase)
  [IO.File]::AppendAllText($phasePath, $Phase + [Environment]::NewLine)
}}
Write-TestPhase 'entry'
function Get-NetFirewallRule {{
  [CmdletBinding()]
  param([string]$DisplayName)
  Write-TestPhase ('get:' + $DisplayName)
  @()
}}
function New-NetFirewallRule {{
  [CmdletBinding()]
  param(
    [string]$DisplayName,
    [string]$Direction,
    [string]$Action,
    [string]$Protocol,
    [int]$LocalPort,
    [string]$LocalAddress,
    [string]$Profile,
    [string]$Enabled,
    [string]$Group,
    [string]$Description
  )
  Write-TestPhase ('new:' + $DisplayName)
}}
Write-TestPhase 'script'
{firewall_script}
Write-TestPhase 'complete'
"#
        );
        let started = Instant::now();
        let output = system_utility::capture(
            SystemUtility::PowerShell,
            &[
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ],
            QUERY_TIMEOUT,
        )
        .unwrap_or_else(|error| {
            panic!(
                "PowerShell test process after {:?}; phases: {:?}; error: {error:?}",
                started.elapsed(),
                std::fs::read_to_string(&phase_path)
            )
        });
        eprintln!(
            "PowerShell fixture completed after {:?}; phases: {:?}",
            started.elapsed(),
            std::fs::read_to_string(&phase_path)
        );

        assert!(
            output.status.success(),
            "{}",
            decode_powershell_text(&output.stderr)
        );
        let results = deserialize_powershell_json_object::<Vec<WindowsFirewallRuleApplyResult>>(
            &output.stdout,
        )
        .expect("firewall result array");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].local_port, 8211);
        assert_eq!(results[0].local_address, "192.168.31.150");
        assert_eq!(results[1].local_port, 27015);
    }
}
