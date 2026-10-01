use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::pseudo_console_transcript::TerminalTranscript;
use super::{
    OwnedWindowsHandle, RuntimeChild, RuntimeStdin, RuntimeStdinCancellation, WindowsSpawnedChild,
};

const WIDTH: usize = 1024;
const HEIGHT: usize = 64;
const INPUT_BUDGET: Duration = Duration::from_secs(2);
const OUTPUT_CLOSE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const ERROR_NO_DATA: i32 = 232;
const ERROR_BROKEN_PIPE: i32 = 109;

#[derive(Debug, Default)]
struct TerminalState {
    closed: AtomicBool,
    output_failed: AtomicBool,
}

/// Both endpoints and the reader belong to one RuntimeChild. The reader drains
/// during ClosePseudoConsole and after its return: Windows 11 24H2 closes the
/// console asynchronously, so the final frame can arrive later.
#[derive(Debug)]
pub(super) struct ManagedPseudoConsole {
    handle: usize,
    state: Arc<TerminalState>,
    reader: Option<JoinHandle<()>>,
}

impl Drop for ManagedPseudoConsole {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe { ClosePseudoConsole(self.handle as *mut c_void) };
        }
        self.state.closed.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[derive(Debug)]
pub(super) struct PseudoConsoleInput {
    pipe: Option<File>,
    state: Arc<TerminalState>,
}

impl PseudoConsoleInput {
    #[cfg(test)]
    pub(super) fn write_line(&mut self, command: &str) -> io::Result<()> {
        self.write_line_with_cancellation(command, &RuntimeStdinCancellation::default())
    }

    pub(super) fn write_line_with_cancellation(
        &mut self,
        command: &str,
        cancellation: &RuntimeStdinCancellation,
    ) -> io::Result<()> {
        self.write_line_with_budget(command, INPUT_BUDGET, cancellation)
    }

    fn write_line_with_budget(
        &mut self,
        command: &str,
        budget: Duration,
        cancellation: &RuntimeStdinCancellation,
    ) -> io::Result<()> {
        if command.len() > 4096 || command.chars().any(char::is_control) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Terminal commands must be a single line of at most 4096 bytes.",
            ));
        }
        let deadline = Instant::now() + budget;
        let mut accepted = 0;
        // The terminal's carriage return shares the command's deadline and
        // cancellation. An accepted body is incomplete until it is delivered.
        for bytes in [command.as_bytes(), b"\r".as_slice()] {
            let mut offset = 0;
            while offset < bytes.len() {
                if self.state.closed.load(Ordering::Acquire) || self.pipe.is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "The managed terminal is no longer available.",
                    ));
                }
                if cancellation.is_cancelled() {
                    if accepted > 0 {
                        self.pipe.take();
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "The terminal input write was cancelled.",
                    ));
                }
                if Instant::now() >= deadline {
                    self.pipe.take();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "The managed terminal did not accept the command before its deadline.",
                    ));
                }
                let Some(pipe) = self.pipe.as_mut() else {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "The managed terminal input is closed.",
                    ));
                };
                match pipe.write(&bytes[offset..]) {
                    Ok(0) => std::thread::sleep(
                        POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                    ),
                    Ok(written) => {
                        offset += written;
                        accepted += written;
                    }
                    Err(error) => {
                        // A partially accepted command must never be joined to
                        // a later command after an I/O failure.
                        self.pipe.take();
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn spawn(
    plan: &super::SpawnCommand<'_>,
    log: impl Write + Send + 'static,
    run_in_background: bool,
    resource_group: Option<&super::RuntimeResourceGroup>,
) -> io::Result<(RuntimeChild, Option<super::WindowsHiddenDesktop>)> {
    super::process_environment::validate_windows_command(plan)?;
    let desktop = run_in_background
        .then(super::create_hidden_desktop_for_spawn)
        .transpose()?;
    let job = super::windows_process_job::OwnedProcessJob::new_in_resource_group(resource_group)?;
    let (input_read, input_write) = create_pipe()?;
    let (output_read, output_write) = create_pipe()?;
    // ConPTY's ends remain synchronous blocking handles. Only the parent's
    // endpoints use PIPE_NOWAIT so writes and reader cleanup have finite waits.
    set_nonblocking(input_write.as_raw())?;
    set_nonblocking(output_read.as_raw())?;
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        CreatePseudoConsole(
            Coord {
                x: WIDTH as i16,
                y: HEIGHT as i16,
            },
            input_read.as_raw(),
            output_write.as_raw(),
            0,
            &mut handle,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "CreatePseudoConsole failed with HRESULT {result:#x}."
        )));
    }
    let state = Arc::new(TerminalState::default());
    let mut terminal = ManagedPseudoConsole {
        handle: handle as usize,
        state: Arc::clone(&state),
        reader: None,
    };
    let output = unsafe { File::from_raw_handle(output_read.into_raw()) };
    let reader_state = Arc::clone(&state);
    terminal.reader = Some(
        std::thread::Builder::new()
            .name("managed-terminal-output".into())
            .spawn(move || drain_output(output, log, reader_state))?,
    );
    let spawn_result =
        spawn_attached_process(plan, handle, job.launch_job_handles(), desktop.as_ref());
    // Release our ConPTY endpoint copies before any error drops the terminal;
    // retaining output_write would prevent its reader from observing EOF.
    drop(input_read);
    drop(output_write);
    let process = spawn_result?;
    super::close_handle(process.thread_handle);
    let input = PseudoConsoleInput {
        pipe: Some(unsafe { File::from_raw_handle(input_write.into_raw()) }),
        state,
    };
    Ok((
        RuntimeChild::Windows(WindowsSpawnedChild {
            process_handle: process.process_handle as usize,
            process_id: process.process_id,
            stdin: Some(RuntimeStdin::PseudoConsole(input)),
            terminal: Some(terminal),
            job: Some(job),
            elevated: None,
            output: None,
        }),
        desktop,
    ))
}

fn spawn_attached_process(
    plan: &super::SpawnCommand<'_>,
    handle: *mut c_void,
    jobs: Box<[*mut c_void]>,
    desktop: Option<&super::WindowsHiddenDesktop>,
) -> io::Result<super::ProcessInformation> {
    let mut attributes = PseudoConsoleAttributes::new(handle, jobs)?;
    let mut desktop_name =
        desktop.map(|desktop| super::wide_null(&super::hidden_desktop_spawn_target(&desktop.name)));
    let mut startup: super::StartupInfoExW = unsafe { std::mem::zeroed() };
    startup.startup_info.cb = std::mem::size_of::<super::StartupInfoExW>() as u32;
    // ConPTY owns the character console, but clients can still create native
    // windows. Keep those windows on the same private desktop as other hosts.
    startup.startup_info.lp_desktop = desktop_name
        .as_mut()
        .map_or(std::ptr::null_mut(), |name| name.as_mut_ptr());
    // Explicit NULL standard handles are essential when LGSM itself was
    // launched with redirected output; otherwise that redirection is inherited.
    startup.startup_info.dw_flags = super::STARTF_USESTDHANDLES;
    startup.attribute_list = attributes.as_mut_raw();
    let mut command_line =
        super::wide_null(&super::build_spawn_command_line(plan.executable, plan.args));
    let mut directory = super::wide_null(&plan.working_directory.to_string_lossy());
    let mut environment = plan.windows_environment();
    let mut process = super::ProcessInformation::default();
    // ConPTY clients also inherit the sender's process-wide Ctrl+C ignore bit.
    // Capture the native error before unlocking; terminal setup/draining stays outside.
    super::windows_console_control::with_windows_console_ctrl_lock(|| {
        let created = unsafe {
            super::CreateProcessW(
                std::ptr::null(),
                command_line.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                super::EXTENDED_STARTUPINFO_PRESENT | super::CREATE_UNICODE_ENVIRONMENT,
                environment
                    .as_mut()
                    .map_or(std::ptr::null_mut(), |block| block.as_mut_ptr().cast()),
                directory.as_mut_ptr(),
                &mut startup.startup_info,
                &mut process,
            )
        };
        if created == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })?;
    Ok(process)
}

fn drain_output(output: File, log: impl Write, state: Arc<TerminalState>) {
    drain_output_with_reader(
        |buffer| read_pipe(&output, buffer),
        log,
        state,
        OUTPUT_CLOSE_BUDGET,
    );
}

fn drain_output_with_reader(
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
    mut log: impl Write,
    state: Arc<TerminalState>,
    close_budget: Duration,
) {
    let mut transcript = TerminalTranscript::new(WIDTH, HEIGHT);
    let mut buffer = [0; 8192];
    let mut close_deadline = None;
    let mut capture_failure = None;
    loop {
        if state.closed.load(Ordering::Acquire) {
            let deadline = close_deadline.get_or_insert_with(|| Instant::now() + close_budget);
            if Instant::now() >= *deadline {
                capture_failure = Some(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "The output drain deadline expired before EOF.",
                ));
                break;
            }
        }
        match read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => {
                if !state.output_failed.load(Ordering::Acquire) {
                    let mut text = String::new();
                    let parsed = transcript.push(&buffer[..size], &mut text);
                    if parsed.is_err() {
                        transcript.drain_pending_rows(&mut text);
                    }
                    let result = persist_transcript(&mut log, &text, parsed);
                    if let Err(error) = result {
                        eprintln!("Managed terminal output could not be persisted: {error}");
                        state.output_failed.store(true, Ordering::Release);
                    }
                }
            }
            Err(error) if error.raw_os_error() == Some(ERROR_NO_DATA) => {
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE) => break,
            Err(error) => {
                capture_failure = Some(error);
                break;
            }
        }
    }
    if !state.output_failed.load(Ordering::Acquire) {
        let mut text = String::new();
        let completed = transcript.finish(&mut text);
        // A broken read or bounded shutdown still owns its decoded final row.
        // Preserve that known prefix before reporting the missing stream tail.
        let completed = capture_failure.map_or(completed, Err);
        let result = persist_transcript(&mut log, &text, completed);
        if let Err(error) = result {
            eprintln!("Managed terminal output could not be completed: {error}");
            state.output_failed.store(true, Ordering::Release);
        }
    }
    if let Err(error) = log.flush() {
        eprintln!("Managed terminal output could not be flushed: {error}");
        state.output_failed.store(true, Ordering::Release);
    }
}

fn persist_transcript(log: &mut impl Write, text: &str, parsed: io::Result<()>) -> io::Result<()> {
    // The caller owns the decoded prefix even when parsing fails later in the
    // same read. Persist it before the failure marker, never the rejected bytes.
    log.write_all(text.as_bytes())?;
    if let Err(error) = parsed {
        record_capture_failure(log, &error);
        return Err(error);
    }
    Ok(())
}

pub(super) fn record_capture_failure(log: &mut impl Write, error: &io::Error) {
    // A parser or pipe failure does not reach the storage sink's write-error
    // state. Record it in the run log so operators and diagnostics cannot
    // mistake an apparently quiet console for a healthy output stream.
    if let Err(recording_error) = writeln!(
        log,
        "\n[LanGame] Terminal output capture stopped: {error} Output capture is incomplete; further output from this console cannot be recorded."
    ) {
        eprintln!("Managed terminal capture failure could not be recorded: {recording_error}");
    }
}

pub(super) fn read_pipe(pipe: &File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    // std::fs::File treats ERROR_NO_DATA as BrokenPipe and maps it to EOF.
    // PIPE_NOWAIT needs the original Win32 code to distinguish idle from closed.
    if unsafe {
        ReadFile(
            pipe.as_raw_handle(),
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(read as usize)
    }
}

pub(super) fn create_pipe() -> io::Result<(OwnedWindowsHandle, OwnedWindowsHandle)> {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { super::CreatePipe(&mut read, &mut write, std::ptr::null_mut(), 65536) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        OwnedWindowsHandle::new(read),
        OwnedWindowsHandle::new(write),
    ))
}

pub(super) fn set_nonblocking(handle: *mut c_void) -> io::Result<()> {
    let mode = 1u32; // PIPE_NOWAIT
    if unsafe { SetNamedPipeHandleState(handle, &mode, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct PseudoConsoleAttributes {
    storage: Vec<usize>,
    jobs: Box<[*mut c_void]>,
}

impl PseudoConsoleAttributes {
    fn new(handle: *mut c_void, jobs: Box<[*mut c_void]>) -> io::Result<Self> {
        let mut size = 0;
        unsafe { super::InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            super::InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut size)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Self { storage, jobs };
        if unsafe {
            super::UpdateProcThreadAttribute(
                attributes.as_mut_raw(),
                0,
                0x00020016,
                handle,
                std::mem::size_of::<*mut c_void>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe {
            super::UpdateProcThreadAttribute(
                attributes.as_mut_raw(),
                0,
                windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                attributes.jobs.as_ptr().cast(),
                std::mem::size_of_val(attributes.jobs.as_ref()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(attributes)
    }

    fn as_mut_raw(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for PseudoConsoleAttributes {
    fn drop(&mut self) {
        unsafe { super::DeleteProcThreadAttributeList(self.as_mut_raw()) };
    }
}

#[repr(C)]
struct Coord {
    x: i16,
    y: i16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadFile(
        pipe: *mut c_void,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn CreatePseudoConsole(
        size: Coord,
        input: *mut c_void,
        output: *mut c_void,
        flags: u32,
        console: *mut *mut c_void,
    ) -> i32;
    fn ClosePseudoConsole(console: *mut c_void);
    fn SetNamedPipeHandleState(
        pipe: *mut c_void,
        mode: *const u32,
        max_collection_count: *const u32,
        collect_data_timeout: *const u32,
    ) -> i32;
}

#[cfg(test)]
#[path = "pseudo_console_failure_tests.rs"]
mod failure_tests;
#[cfg(test)]
#[path = "pseudo_console_tests.rs"]
mod tests;
