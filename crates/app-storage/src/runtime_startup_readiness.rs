use super::{
    ark_readiness, dragonwilds_health, nightingale_health, sonsoftheforest_health, soulmask_health,
    squad_health, theforest_health, unturned_health, windrose_health,
};

const MAX_LINE_BYTES: usize = 64 * 1024;

/// Reused native files can replay earlier output after rotation or replacement.
/// Timestamped Unreal records must belong to the captured OS generation; an
/// undated line still relies on the caller's pre-spawn byte cursor.
pub fn startup_log_line_is_current(identity: &app_core::ProcessIdentity, line: &str) -> bool {
    let Some(observed) = ark_readiness::native_line_timestamp(line.trim()) else {
        return true;
    };
    const WINDOWS_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    identity
        .creation_time
        .checked_sub(WINDOWS_UNIX_EPOCH_TICKS)
        // Preserve the existing millisecond precision of native Unreal output.
        .is_some_and(|ticks| observed >= ticks / 10_000)
}

/// Incremental readiness of one startup's native log. The caller owns process
/// identity, log cursors, complete-line framing, liveness and cancellation.
/// No retained run, database row or historical log is consulted here.
pub struct StartupLogReadiness {
    module_id: &'static str,
    ready: bool,
    failed: bool,
    squad: squad_health::Readiness,
    soulmask: soulmask_health::Readiness,
    unturned: unturned_health::Readiness,
    valheim: ValheimReadiness,
}

impl StartupLogReadiness {
    pub fn new(module_id: &str) -> Result<Self, String> {
        let module_id = match module_id.trim().to_ascii_lowercase().as_str() {
            "abioticfactor" => "abioticfactor",
            "arksurvivalascended" => "arksurvivalascended",
            "arksurvivalevolved" => "arksurvivalevolved",
            "barotrauma" => "barotrauma",
            "conanexiles" => "conanexiles",
            "corekeeper" => "corekeeper",
            "enshrouded" => "enshrouded",
            "humanitz" => "humanitz",
            "minecraft" => "minecraft",
            "necesse" => "necesse",
            "nightingale" => "nightingale",
            "palworld" => "palworld",
            "projectzomboid" => "projectzomboid",
            "rimworld" => "rimworld",
            "romestead" => "romestead",
            "runescapedragonwilds" => "runescapedragonwilds",
            "rust" => "rust",
            "satisfactory" => "satisfactory",
            "scum" => "scum",
            "sevendaystodie" => "sevendaystodie",
            "sonsoftheforest" => "sonsoftheforest",
            "soulmask" => "soulmask",
            "squad" => "squad",
            "terraria" => "terraria",
            "theforest" => "theforest",
            "unturned" => "unturned",
            "valheim" => "valheim",
            "vrising" => "vrising",
            "windrose" => "windrose",
            _ => {
                return Err(format!(
                    "Module {module_id} has no startup log readiness contract"
                ));
            }
        };
        Ok(Self {
            module_id,
            ready: false,
            failed: false,
            squad: Default::default(),
            soulmask: Default::default(),
            unturned: Default::default(),
            valheim: Default::default(),
        })
    }

    pub fn observe(&mut self, line: &str) {
        if line.len() > MAX_LINE_BYTES {
            return;
        }
        let line = line.trim();
        if line.contains(['\n', '\r', '\0'])
            // JSON formats require a verified message/category envelope. Never
            // search arbitrary property values (such as a configured name).
            || line.starts_with('{')
            || (self.module_id != "theforest"
                && (line.starts_with("[LanGame ") || line.starts_with("[LanGame]")))
        {
            return;
        }
        if !crate::runtime_fatal_log_lines(self.module_id, &[line.to_owned()]).is_empty() {
            self.failed = true;
        }
        // ASA's shared native filename can retain prior runs. Unlike the
        // legacy health fixtures, startup requires its native timestamp and
        // the caller must check that timestamp against the process identity.
        if self.module_id == "arksurvivalascended"
            && ark_readiness::ark_native_log_message(line).is_none()
        {
            return;
        }
        match self.module_id {
            "enshrouded" => {
                if let Some(ready) = super::enshrouded_session_state(line) {
                    self.ready = ready;
                }
            }
            "runescapedragonwilds" => match dragonwilds_health::event(line) {
                Some(dragonwilds_health::Event::Ready(ready)) => self.ready = ready,
                Some(
                    dragonwilds_health::Event::Fatal | dragonwilds_health::Event::OwnerInvalid,
                ) => {
                    self.failed = true;
                }
                None => {}
            },
            "squad" => {
                if let Some(event) = squad_health::event(line) {
                    self.squad.observe(event);
                }
                self.ready = self.squad.is_ready();
            }
            "soulmask" => {
                if let Some(event) = soulmask_health::event(line) {
                    self.soulmask.observe(event);
                }
                self.ready = self.soulmask.is_ready();
            }
            "unturned" => {
                if let Some(event) = unturned_health::event(line) {
                    self.unturned.observe(event);
                }
                self.ready = self.unturned.is_ready();
            }
            "sonsoftheforest" => match sonsoftheforest_health::event(line) {
                Some(sonsoftheforest_health::Event::Ready) => self.ready = true,
                Some(sonsoftheforest_health::Event::Starting) => self.ready = false,
                Some(sonsoftheforest_health::Event::Fatal) => self.failed = true,
                None => {}
            },
            "theforest" => {
                if let Some(event) = theforest_health::event(line) {
                    self.ready = matches!(event, theforest_health::Event::Ready);
                }
            }
            "nightingale" => {
                if let Some(event) = nightingale_health::event(line) {
                    self.ready = matches!(event, nightingale_health::Event::Ready);
                }
            }
            "windrose" => {
                if let Some(stage) = windrose_health::windrose_native_stage(line) {
                    self.ready = matches!(stage, windrose_health::WindroseNativeStage::HostReady);
                }
            }
            "valheim" => {
                self.valheim.observe_line(line);
                self.ready = self.valheim.is_ready();
            }
            _ => {
                if simple_ready_signal(self.module_id, line) {
                    self.ready = true;
                }
                if self.module_id == "abioticfactor"
                    && super::analyze_abiotic_factor_runtime_health(&[line.to_owned()])
                        .is_some_and(|health| health.status == "error")
                {
                    self.failed = true;
                }
                if self.module_id == "projectzomboid"
                    && line
                        .contains("WorldDictionary: Cannot load world due to WorldDictionary error")
                {
                    self.failed = true;
                }
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready && !self.failed
    }

    pub(super) fn valheim_state(&self) -> Option<ValheimReadiness> {
        (self.module_id == "valheim").then(|| self.valheim.clone())
    }

    pub fn has_failed(&self) -> bool {
        self.failed
    }

    /// Begin a new log generation; no readiness survives rotation/replacement.
    pub fn reset(&mut self) {
        self.ready = false;
        self.failed = false;
        self.squad = Default::default();
        self.soulmask = Default::default();
        self.unturned = Default::default();
        self.valheim = Default::default();
    }
}

pub(super) fn simple_ready_signal(module_id: &str, line: &str) -> bool {
    let line = line.trim();
    match module_id {
        "arksurvivalascended" => ark_readiness::has_asa_ready_marker(&line.to_ascii_lowercase()),
        "arksurvivalevolved" => unreal_message(line).is_some_and(ase_full_startup),
        "conanexiles" => unreal_record_message(line)
            == Some("Entered application state 'WorldPersistenceDone'"),
        "rust" => line == "Server startup complete",
        "abioticfactor" => unreal_message(line)
            .map(|message| message.strip_prefix("LogAbiotic: Warning: ").unwrap_or(message))
            .and_then(|message| message.strip_prefix("Session short code:"))
            .is_some_and(|code| !code.trim().is_empty()),
        "corekeeper" => line == "timescale = 0",
        "barotrauma" => line == "Server started",
        // Terraria starts its input thread before printing readiness. Its
        // native Console.Write(": ") prompt can share that completed row.
        "terraria" => matches!(line, "Server started" | ": Server started"),
        "romestead" => line == "Server ready",
        "minecraft" => super::clock_log_message(line)
            .and_then(|message| message.strip_prefix(" [Server thread/INFO]: Done ("))
            .and_then(|message| message.strip_suffix("s)! For help, type \"help\""))
            .and_then(|elapsed| elapsed.parse::<f64>().ok())
            .is_some_and(|elapsed| elapsed.is_finite() && elapsed >= 0.0),
        "necesse" => line
            .strip_prefix("Started server using port ")
            .and_then(|message| message.split_once(" with "))
            .is_some_and(|(port, slots)| {
                valid_port(port)
                    && slots
                        .strip_suffix(" slots")
                        .and_then(|slots| slots.parse::<u32>().ok())
                        .is_some_and(|slots| slots > 0)
            }),
        "humanitz" => unreal_message(line)
            == Some("LogHZSuccess: Display: Success => Session created!"),
        // Palworld build 25247047 (runtime-fixtures/2026-10-01-stdout-device.json)
        // prints this line directly with vfwprintf(stdout), independently of
        // LogFormatType / -logformat=Json: ready RVA 0x02dfbccd -> 0x032823c0
        // -> 0x00b2b250. It remains plain text within the managed JSON stream.
        "palworld" => unreal_message(line)
            .and_then(|message| message.strip_prefix("Running Palworld dedicated server on "))
            .is_some_and(palworld_endpoint),
        "projectzomboid" => zomboid_message(line) == Some("*** SERVER STARTED ****"),
        "rimworld" => super::clock_log_message(line)
            .and_then(|message| message.strip_prefix(" | Listening for users at "))
            .and_then(|endpoint| endpoint.parse::<std::net::SocketAddr>().ok())
            .is_some_and(|endpoint| endpoint.port() != 0),
        "satisfactory" => unreal_message(line).is_some_and(|message| {
            message
                .strip_prefix("LogServer: Display: Server startup time elapsed and saving/level loading is done")
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(", "))
        }),
        "scum" => unreal_message(line)
            .and_then(|message| message.strip_prefix("LogSCUM: Global Stats:"))
            .is_some_and(|stats| !stats.trim().is_empty()),
        "sevendaystodie" => seven_days_message(line) == Some("INF StartGame done"),
        "valheim" => valheim_message(line) == Some("Opened Steam server"),
        "vrising" => line == "[Server] Startup Completed - Disabling Scene Loading Systems",
        _ => false,
    }
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

fn ase_full_startup(message: &str) -> bool {
    let Some(elapsed) = message.strip_prefix("Full Startup: ") else {
        return false;
    };
    let Some((elapsed, suffix)) = elapsed.split_once(" seconds") else {
        return false;
    };
    let duration = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && value
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value >= 0.0)
    };
    duration(elapsed)
        && (suffix.is_empty()
            || suffix
                .strip_prefix(" (BP compile: ")
                .and_then(|value| value.strip_suffix(" seconds)"))
                .is_some_and(duration))
}

fn unreal_record_message(line: &str) -> Option<&str> {
    let message = unreal_message(line)?;
    let Some((category, rest)) = message.split_once(':') else {
        return Some(message);
    };
    // Accept an Unreal category, optionally followed by its native verbosity.
    // A configured name, arbitrary property or surrounding sentence is not a
    // log envelope. In particular, never search inside JSON values.
    if category.is_empty()
        || !category
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !category.as_bytes()[0].is_ascii_alphabetic()
    {
        return None;
    }
    let rest = rest.trim_start();
    if let Some((level, body)) = rest.split_once(':')
        && matches!(
            level,
            "Fatal" | "Error" | "Warning" | "Display" | "Log" | "Verbose" | "VeryVerbose"
        )
    {
        return Some(body.trim_start());
    }
    category.starts_with("Log").then_some(rest)
}

fn palworld_endpoint(endpoint: &str) -> bool {
    // The native format is "%s:%d". An unspecified host is empty; configured
    // IPv4/IPv6 bindings must not be mistaken for a different log message.
    if let Ok(endpoint) = endpoint.parse::<std::net::SocketAddr>() {
        return endpoint.port() != 0;
    }
    endpoint.rsplit_once(':').is_some_and(|(host, port)| {
        valid_port(port) && (host.is_empty() || host.parse::<std::net::IpAddr>().is_ok())
    })
}

fn unreal_message(line: &str) -> Option<&str> {
    if line.starts_with('[') {
        ark_readiness::ark_native_log_message(line).map(|(_, message)| message)
    } else {
        Some(line)
    }
}

fn seven_days_message(line: &str) -> Option<&str> {
    if line.starts_with("INF ") {
        return Some(line);
    }
    let (timestamp, rest) = line.split_once(' ')?;
    if timestamp.len() != 19
        || !timestamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                _ => byte.is_ascii_digit(),
            })
    {
        return None;
    }
    let (elapsed, message) = rest.trim_start().split_once(' ')?;
    elapsed
        .parse::<f64>()
        .ok()
        .filter(|elapsed| elapsed.is_finite() && *elapsed >= 0.0)
        .map(|_| message.trim_start())
}

fn valheim_message(line: &str) -> Option<&str> {
    if !line.bytes().next()?.is_ascii_digit() {
        return Some(line);
    }
    let (timestamp, message) = line.split_once(": ")?;
    timestamp
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'/' | b':' | b' '))
        .then_some(message)
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ValheimReadiness {
    steam: bool,
    playfab_world: bool,
    playfab_active: bool,
}

impl ValheimReadiness {
    pub(super) fn observe_line(&mut self, line: &str) {
        if line.len() <= MAX_LINE_BYTES
            && !line.contains(['\r', '\n', '\0'])
            && let Some(message) = valheim_message(line.trim())
        {
            self.observe(message);
        }
    }

    fn observe(&mut self, message: &str) {
        // The native assembly and ZNet::OnGenerationFinished/OpenServer IL are bound by
        // modules/valheim/reference-fixtures/2026-10-30-native_build_26266951.json.
        // PlayFab StartHost only sets a listen object. ZPlayFabMatchmaking's
        // OnActivateLobbySuccess sets State::Active (3); OnSessionUpdated then
        // emits the exact active-session message at IL_0077.
        match message {
            "Opened Steam server" => {
                self.steam = true;
                self.playfab_world = false;
                self.playfab_active = false;
            }
            "Opened PlayFab server" => {
                self.steam = false;
                if self.playfab_world {
                    self.playfab_active = false;
                }
                self.playfab_world = true;
            }
            _ if playfab_session_is_active(message) => self.playfab_active = true,
            // Actual failure/reconnect records, not arbitrary text containing a
            // name or token. Tokens are inspected transiently and never retained.
            // https://github.com/ptitSeb/box64/issues/4403
            // https://www.reddit.com/r/valheim/comments/1vi91a6/deactivated_playfab_lobby/
            _ if message.starts_with("PlayFab reconnect server '")
                || message.starts_with(
                    "ZPlayFabMatchmaking::UnregisterServer - unregistering server now.",
                )
                || message.starts_with("Unregister PlayFab server \"")
                || message.starts_with("Deactivated PlayFab lobby ")
                || message.starts_with("Left PlayFab lobby ") =>
            {
                self.playfab_active = false;
            }
            _ => {}
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.steam || (self.playfab_world && self.playfab_active)
    }
}

fn playfab_session_is_active(message: &str) -> bool {
    let Some((name, status)) = message
        .strip_prefix("Session \"")
        .and_then(|rest| rest.rsplit_once("\" with join code "))
    else {
        return false;
    };
    let Some((code, status)) = status.split_once(" and IP ") else {
        return false;
    };
    let Some((endpoint, players)) = status.split_once(" is active with ") else {
        return false;
    };
    !name.is_empty()
        && code.len() == 6
        && code.bytes().all(|byte| byte.is_ascii_digit())
        // OnSessionUpdated IL_00b8 substitutes this exact sentinel when public
        // IP discovery has not completed. The lobby is already Active, and a
        // later RefreshLobby IP update does not emit another active message.
        && (endpoint == "<RESOLVING>"
            || endpoint
                .parse::<std::net::SocketAddr>()
                .is_ok_and(|endpoint| endpoint.port() != 0))
        && players.strip_suffix(" player(s)").is_some_and(|players| {
            !players.is_empty()
                && players.bytes().all(|byte| byte.is_ascii_digit())
                && players.parse::<u32>().is_ok()
        })
}

fn zomboid_message(line: &str) -> Option<&str> {
    if line == "*** SERVER STARTED ****" {
        return Some(line);
    }
    let (prefix, message) = line.split_once('>')?;
    let prefix = prefix
        .strip_prefix("LOG")?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let (category, metadata) = prefix.split_once(char::is_whitespace)?;
    (matches!(category, "Network" | "General")
        && metadata
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ' ' | ',' | '.' | ':' | 'f' | 's' | 't')))
    .then_some(message.trim())
}

#[cfg(test)]
#[path = "runtime_startup_readiness_tests.rs"]
mod tests;
