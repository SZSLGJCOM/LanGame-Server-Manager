use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Context, ControlSnapshot, Operation, io, response};

static IN_FLIGHT: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

struct Lease(PathBuf);
impl Lease {
    fn acquire(root: &Path) -> Result<Self, String> {
        let key =
            dunce::canonicalize(root).map_err(|_| "Cannot identify the Valheim control root.")?;
        let mut active = IN_FLIGHT
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !active.insert(key.clone()) {
            return Err("Another Valheim control request is already in progress.".into());
        }
        Ok(Self(key))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        IN_FLIGHT
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

fn verify_process(context: &Context) -> Result<(), String> {
    if app_runtime::process_matches_identity(context.pid, &context.identity)
        .map_err(|_| "Cannot verify the managed Valheim process.")?
    {
        Ok(())
    } else {
        Err("The managed Valheim process changed or stopped.".into())
    }
}

struct OpenControl {
    _pins: Vec<File>,
    token: String,
}

fn open(context: &Context, payload: &[u8]) -> Result<OpenControl, String> {
    let mut pins = io::pin_directories(&context.root)?;
    pins.push(io::pin_directory(&context.root.join(io::DIRECTORY))?);
    pins.push(io::pin_directory(&context.root.join("BepInEx"))?);
    pins.push(io::pin_directory(&context.root.join("BepInEx/plugins"))?);
    let (owner, owner_bytes) = io::pin_regular(&context.root.join(io::OWNER), 4096)?;
    io::verify_owner(&owner_bytes, payload)?;
    pins.push(owner);
    let (plugin, plugin_bytes) =
        io::pin_regular(&context.root.join(io::PLUGIN), io::MAX_PLUGIN_BYTES)?;
    if io::digest(&plugin_bytes) != io::digest(payload) {
        return Err("The installed Valheim extension does not match the bundled payload.".into());
    }
    pins.push(plugin);
    let (session_file, session_bytes) =
        io::pin_regular(&context.root.join(io::DIRECTORY).join(io::SESSION), 4096)?;
    let session = io::parse_session(&session_bytes)?;
    pins.push(session_file);
    Ok(OpenControl {
        _pins: pins,
        token: session.token,
    })
}

#[derive(Serialize)]
struct Request<'a> {
    protocol: u32,
    nonce: &'a str,
    token: &'a str,
    op: Operation,
}

pub(super) fn exchange(context: &Context, operation: Operation) -> Result<ControlSnapshot, String> {
    exchange_with(
        context,
        operation,
        io::bundled_payload(),
        Duration::from_secs(10),
        || verify_process(context),
    )
}

// The injected verifier and budget let tests exercise the real file boundary
// without starting a game. Production always uses OS process identity above.
pub(super) fn exchange_with(
    context: &Context,
    operation: Operation,
    payload: &[u8],
    timeout: Duration,
    verify: impl Fn() -> Result<(), String>,
) -> Result<ControlSnapshot, String> {
    verify()?;
    let _lease = Lease::acquire(&context.root)?;
    let control = open(context, payload)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let request = serde_json::to_vec(&Request {
        protocol: 1,
        nonce: &nonce,
        token: &control.token,
        op: operation,
    })
    .map_err(|_| "Cannot encode the Valheim control request.")?;
    verify()?;
    let directory = context.root.join(io::DIRECTORY);
    io::publish(&directory.join("request.json"), &request)?;
    // Never resend a request after publication, especially a reload whose
    // reply can be lost after its side effect has already completed.
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        verify()
            .map_err(|_| "The Valheim process changed after submission; the outcome is unknown.")?;
        if let Some(body) = io::read_optional(
            &directory.join("response.json"),
            response::MAX_RESPONSE_BYTES,
        )
        .map_err(unknown_outcome)?
        {
            // A previous correlated response remains until native publication.
            // Only a well-shaped old nonce may be ignored; malformed packets
            // and matching-nonce schema violations are explicit failures.
            if matching_nonce(&body, &nonce).map_err(unknown_outcome)? {
                let packet =
                    response::parse(&body, &nonce, context.pid, operation).map_err(|error| {
                        let message = error.to_string().replace(&control.token, "[redacted]");
                        match error {
                            response::ResponseError::Rejected(_) => message,
                            _ => unknown_outcome(message),
                        }
                    })?;
                verify()
                    .map_err(|_| "The Valheim process changed before its reply was confirmed.")?;
                return Ok(packet);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("No confirmed Valheim control reply arrived; the operation outcome is unknown. Refresh before another change.".into())
}

fn unknown_outcome(message: String) -> String {
    format!("{message} The operation outcome is unknown. Refresh before another change.")
}

fn matching_nonce(body: &[u8], expected: &str) -> Result<bool, String> {
    // Avoid accepting duplicate nonce fields through serde_json::Value's
    // overwrite semantics. All remaining fields are validated by typed parse.
    #[derive(serde::Deserialize)]
    struct Correlation {
        nonce: String,
    }
    let correlation: Correlation = serde_json::from_slice(body)
        .map_err(|_| "The Valheim control reply has invalid request correlation.")?;
    if !io::nonce(&correlation.nonce) {
        return Err("The Valheim control reply has invalid request correlation.".into());
    }
    Ok(correlation.nonce == expected)
}
