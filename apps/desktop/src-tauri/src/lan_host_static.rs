//! Static LAN assets are confined to the distribution root and streamed in bounded chunks.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};

use super::{
    ResponseSpec, ResponseWritePolicy, ShutdownCheck, ensure_response_write_active, flush_response,
    write_response, write_response_bytes,
};

const COPY_BUFFER_BYTES: usize = 64 * 1024;

pub(super) fn serve_static_file(
    stream: &mut TcpStream,
    dist_dir: &Path,
    request_path: &str,
    head_only: bool,
    shutdown: &ShutdownCheck,
) -> Result<(), String> {
    let policy = ResponseWritePolicy::connection(shutdown);
    let Some(relative) = relative_path(request_path) else {
        return write_response(stream, ResponseSpec::new(404, "text/plain", b""), policy);
    };
    let root =
        fs::canonicalize(dist_dir).map_err(|error| format!("resolve LAN assets: {error}"))?;
    let mut path = root.join(relative);
    let mut file = match File::open(&path) {
        Ok(file) if file.metadata().is_ok_and(|metadata| metadata.is_file()) => file,
        Ok(_) => {
            path = root.join("index.html");
            File::open(&path).map_err(|error| format!("open LAN index: {error}"))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound || path.is_dir() => {
            path = root.join("index.html");
            File::open(&path).map_err(|error| format!("open LAN index: {error}"))?
        }
        Err(error) => return Err(format!("open LAN asset: {error}")),
    };
    // Validate the opened handle, including directory links; checking only the
    // requested spelling would allow a linked asset to disclose arbitrary files.
    let resolved =
        opened_path(&file, &path).map_err(|error| format!("resolve opened LAN asset: {error}"))?;
    if !resolved.starts_with(&root) {
        return write_response(stream, ResponseSpec::new(404, "text/plain", b""), policy);
    }
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect LAN asset: {error}"))?;
    if !metadata.is_file() {
        return Err("LAN asset is not a regular file".into());
    }
    write_response(
        stream,
        ResponseSpec::head(200, content_type_for_path(&path), metadata.len()),
        policy,
    )?;
    if !head_only {
        copy_body(&mut file, stream, metadata.len(), policy)?;
        flush_response(stream, policy)?;
    }
    Ok(())
}

fn copy_body(
    source: &mut impl Read,
    stream: &mut impl Write,
    mut remaining: u64,
    policy: ResponseWritePolicy<'_>,
) -> Result<(), String> {
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        ensure_response_write_active(policy)?;
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = source
            .read(&mut buffer[..limit])
            .map_err(|error| format!("read LAN asset: {error}"))?;
        if count == 0 {
            return Err("LAN asset changed during transfer".into());
        }
        write_response_bytes(stream, &buffer[..count], policy)?;
        remaining -= count as u64;
    }
    Ok(())
}

fn relative_path(request: &str) -> Option<PathBuf> {
    let raw = request.split('?').next()?;
    if !raw.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(raw)?;
    // ':' selects NTFS alternate streams; backslashes and control characters
    // are not accepted as alternate URL path separators or filesystem syntax.
    if decoded
        .chars()
        .any(|ch| ch == ':' || ch == '\\' || ch.is_control())
    {
        return None;
    }
    let path = Path::new(decoded.trim_start_matches('/'));
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(if path.as_os_str().is_empty() {
        PathBuf::from("index.html")
    } else {
        path.to_owned()
    })
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = value.bytes();
    let mut output = Vec::with_capacity(value.len());
    while let Some(byte) = bytes.next() {
        output.push(if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            ((high << 4) | low) as u8
        } else {
            byte
        });
    }
    String::from_utf8(output).ok()
}

#[cfg(windows)]
fn opened_path(file: &File, _path: &Path) -> std::io::Result<PathBuf> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let mut buffer = vec![0_u16; 32_768];
    // The owned file keeps the queried handle valid through the call.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if length == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(std::io::Error::other(
            "LAN asset path exceeds the supported length",
        ));
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..length],
    )))
}

#[cfg(not(windows))]
fn opened_path(_file: &File, path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path)
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[path = "lan_host_static_tests.rs"]
mod tests;
