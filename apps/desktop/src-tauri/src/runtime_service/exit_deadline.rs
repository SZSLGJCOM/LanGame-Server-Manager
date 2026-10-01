use app_runtime::ProcessExitTarget;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// One allowance for the whole request, including IPC, saves and background work.
pub(super) const EXIT_GRACE_TIMEOUT: Duration = Duration::from_secs(120);
static SERVICE_EXIT_ARMED: AtomicBool = AtomicBool::new(false);

pub(super) fn deadline_tick_ms(grace: Duration) -> u64 {
    system_tick_ms().saturating_add(grace.min(EXIT_GRACE_TIMEOUT).as_millis() as u64)
}

fn system_tick_ms() -> u64 {
    // The Windows boot-relative clock has the same origin in both processes.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

fn remaining_budget(deadline_tick_ms: u64, now_tick_ms: u64) -> Result<Duration, String> {
    let remaining = Duration::from_millis(deadline_tick_ms.saturating_sub(now_tick_ms));
    if remaining > EXIT_GRACE_TIMEOUT {
        return Err(String::from(
            "Final exit deadline exceeds the 120-second limit",
        ));
    }
    Ok(remaining)
}

pub(super) fn local_deadline(deadline_tick_ms: u64) -> Result<Instant, String> {
    remaining_budget(deadline_tick_ms, system_tick_ms()).map(|remaining| Instant::now() + remaining)
}

/// The service also owns a deadline after accepting a final exit, so losing the
/// desktop process cannot leave an already accepted shutdown waiting forever.
pub(super) fn arm_service(deadline_tick_ms: u64) -> Result<(), String> {
    let remaining = remaining_budget(deadline_tick_ms, system_tick_ms())?;
    if SERVICE_EXIT_ARMED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let deadline = Instant::now() + remaining;
    if std::thread::Builder::new()
        .name(String::from("service-final-exit"))
        .spawn(move || {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            terminate_current_process(1);
        })
        .is_err()
    {
        terminate_current_process(1);
    }
    Ok(())
}

pub(super) fn arm(target: Arc<ProcessExitTarget>, deadline: Instant) {
    let emergency_target = Arc::clone(&target);
    if app_runtime::spawn_exit_watchdog(target, deadline, |outcome| {
        // This is the final owner of the exit decision. Logging, Tauri event
        // dispatch, Tokio scheduling and application locks must not gate it.
        terminate_current_process(i32::from(outcome.forced || outcome.error.is_some()));
    })
    .is_err()
    {
        // Failure to create a watchdog means no delayed guarantee is possible.
        // Retained kernel handles still allow final cleanup without PID lookup.
        let _ = emergency_target.terminate();
        terminate_current_process(1);
    }
}

/// A confirmed independent owner now protects the runtime. A frozen interface
/// event loop must not turn background saving into another visible exit wait.
pub(super) fn finish_interface() {
    if std::thread::Builder::new()
        .name("interface-final-exit".into())
        .spawn(|| {
            std::thread::sleep(Duration::from_millis(500));
            terminate_current_process(0);
        })
        .is_err()
    {
        terminate_current_process(0);
    }
}

pub(super) fn terminate_current_process(code: i32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // SAFETY: The pseudo-handle identifies this process only. Runtime owners
    // reach this at their deadline; an interface reaches it only after another
    // process has accepted ownership of the runtime and its remaining work.
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    // Do not run potentially blocked exit handlers if the native call failed.
    std::process::abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_service_delivery_consumes_the_original_budget() {
        assert_eq!(
            remaining_budget(121_000, 41_000).unwrap(),
            Duration::from_secs(80)
        );
        assert_eq!(remaining_budget(121_000, 121_001).unwrap(), Duration::ZERO);
        assert!(remaining_budget(121_001, 1_000).is_err());
    }
}
