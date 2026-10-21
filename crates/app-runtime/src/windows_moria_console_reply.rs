//! Bounded private helper framing and completion; no game input is sent here.
use super::*;

pub(super) fn read_reply(
    child: &mut OwnedHelper,
    output: &File,
    deadline: Instant,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        // Observe exit before checking the pipe, so a reply written immediately
        // before exit is drained rather than lost in an exit/empty-read race.
        let status = child.process.try_wait()?;
        let mut available = 0;
        // std's anonymous stdout read handle has GENERIC_READ, without the
        // FILE_WRITE_ATTRIBUTES required to change a read-only pipe's mode.
        // This is the same peek-then-read strategy as the player helper. There
        // is one reader; ReadFile never requests more than the available bytes.
        if unsafe {
            PeekNamedPipe(
                output.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let failure = std::io::Error::last_os_error();
            if failure.raw_os_error() != Some(109) {
                return Err(failure);
            }
        } else if available > 0 {
            let mut buffer = [0u8; 1024];
            let length = (available as usize).min(buffer.len());
            if bytes.len() + length > MAX_REPLY {
                return Err(std::io::Error::other("reply limit"));
            }
            let mut count = 0;
            if unsafe {
                ReadFile(
                    output.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    length as u32,
                    &mut count,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            bytes.extend_from_slice(&buffer[..count as usize]);
            continue;
        }
        if let Some(status) = status {
            return if status.success() {
                Ok(bytes)
            } else {
                Err(std::io::Error::other("helper failed"))
            };
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "helper deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn parse_reply(
    bytes: &[u8],
    nonce: &str,
    root: u32,
    target: u32,
) -> Result<&'static str, RuntimeProcessError> {
    let invalid = || {
        error(
            target,
            "Moria exit helper reply is invalid or unknown; no retry",
        )
    };
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut replies = text.lines().filter_map(|line| line.strip_prefix(FRAME));
    let value: Value =
        serde_json::from_str(replies.next().ok_or_else(invalid)?).map_err(|_| invalid())?;
    if replies.next().is_some()
        || !keys(
            &value,
            &["nonce", "root", "target", "queued", "input_read", "error"],
        )
        || value["nonce"].as_str() != Some(nonce)
        || value["root"].as_u64() != Some(root.into())
        || value["target"].as_u64() != Some(target.into())
    {
        return Err(invalid());
    }
    if value["queued"] == true && value["error"].is_null() {
        return match value["input_read"].as_str() {
            Some("cancel_requested") => Ok("cancel_requested"),
            Some("no_pending_read") => Ok("no_pending_read"),
            Some("input_thread_exited") => Ok("input_thread_exited"),
            Some("native_process_exited") => Ok("native_process_exited"),
            _ => Err(invalid()),
        };
    }
    if value["queued"] == false
        && value["input_read"].is_null()
        && let Some(message) = value["error"]
            .as_str()
            .filter(|message| message.len() <= 1024 && !message.chars().any(char::is_control))
    {
        return Err(error(
            target,
            format!("Moria exit helper: {message}; no retry"),
        ));
    }
    Err(invalid())
}
