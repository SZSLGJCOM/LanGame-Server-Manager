//! Validates public web links before handing them to the operating system.

const MAX_URL_BYTES: usize = 8192;
const INVALID_URL: &str = "Only absolute http(s) URLs without credentials are allowed.";

fn parse_external_url(input: &str) -> Result<reqwest::Url, String> {
    // WHATWG parsing repairs control characters and backslashes. Reject these
    // spellings instead of allowing the shell and the UI to interpret them differently.
    if input.chars().any(|ch| ch.is_control() || ch == '\\') {
        return Err(INVALID_URL.into());
    }
    let trimmed = input.trim();
    let (scheme, authority) = trimmed.split_once("://").ok_or(INVALID_URL)?;
    if trimmed.len() > MAX_URL_BYTES
        || (!scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https"))
        || authority.starts_with('/')
    {
        return Err(INVALID_URL.into());
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| INVALID_URL)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > MAX_URL_BYTES
    {
        return Err(INVALID_URL.into());
    }
    Ok(url)
}

pub(super) async fn open(input: &str) -> Result<(), String> {
    let url = parse_external_url(input)?;
    #[cfg(windows)]
    {
        // A shell extension can block. Keep it off the WebView thread, admit
        // only one dispatch, and retain admission even if the caller disconnects.
        static OPEN_SLOT: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
            std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)));
        let permit = std::sync::Arc::clone(&OPEN_SLOT)
            .try_acquire_owned()
            .map_err(|_| "An external link is already being opened.")?;
        tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            windows::open(&url)
        })
        .await
        .map_err(|error| format!("External link dispatch failed: {error}"))?
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("Opening external URLs is unsupported on this platform.".into())
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
    const SW_SHOWNORMAL: i32 = 1;

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            // SAFETY: constructed after a successful initialization on this same thread.
            unsafe { CoUninitialize() };
        }
    }

    pub(super) fn open(url: &reqwest::Url) -> Result<(), String> {
        // Shell extensions may require STA COM. Every successful call, including
        // S_FALSE, must be balanced before returning this blocking worker to its pool.
        let initialized = unsafe {
            CoInitializeEx(
                null_mut(),
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            )
        };
        if initialized < 0 {
            return Err(format!(
                "Initialize external link dispatch failed (HRESULT {initialized:#010x})."
            ));
        }
        let _apartment = ComApartment;
        let target: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        // SAFETY: null-terminated UTF-16 values outlive this call. Validation
        // restricts the target to HTTP(S); no executable or command line is supplied.
        let result = unsafe {
            ShellExecuteW(
                null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if result <= 32 {
            return Err(format!(
                "Windows could not open the external link (error {result})."
            ));
        }
        Ok(())
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            window: *mut c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> *mut c_void;
    }
}

#[cfg(test)]
#[path = "commands_external_url_tests.rs"]
mod tests;
