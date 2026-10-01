//! Media transfers leave the four management workers as soon as headers are read.
//! A separate bounded admission permit owns the async read and blocking socket write.
use super::*;
use crate::media_cache::{MediaCacheState, error_response};
use tauri::http::Response;

pub(super) fn dispatch(
    mut stream: TcpStream,
    app: tauri::AppHandle,
    request: HttpRequestHead,
    shutdown: ShutdownCheck,
) -> Result<(), String> {
    if request.method != "GET" && request.method != "HEAD" {
        return write_media_response(
            &mut stream,
            error_response(405, "Method not allowed"),
            &shutdown,
        );
    }
    let Some(state) = app
        .try_state::<MediaCacheState>()
        .map(|state| state.inner().clone())
    else {
        return write_media_response(
            &mut stream,
            error_response(503, "Media cache is unavailable"),
            &shutdown,
        );
    };
    // This scoped lease was issued by the authenticated management API. The
    // management token itself is never placed in an image/video URL.
    let Some(source) = state.lookup(&request.path) else {
        return write_media_response(
            &mut stream,
            error_response(404, "Media reference expired"),
            &shutdown,
        );
    };
    let Some(permit) = state.admit() else {
        return write_media_response(
            &mut stream,
            error_response(503, "Media cache is busy"),
            &shutdown,
        );
    };
    let range = request.headers.get("range").cloned();
    let head = request.method == "HEAD";
    tauri::async_runtime::spawn(async move {
        let response = state
            .respond(source, range, head, Arc::clone(&shutdown))
            .await;
        // The permit remains held through the socket write; slow peers cannot
        // create an unlimited queue of response bodies or blocking writers.
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            write_media_response(&mut stream, response, &shutdown)
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("LAN media response ended: {error}"),
            Err(error) => eprintln!("LAN media response worker failed: {error}"),
        }
    });
    Ok(())
}

fn write_media_response(
    stream: &mut TcpStream,
    response: Response<Vec<u8>>,
    shutdown: &ShutdownCheck,
) -> Result<(), String> {
    let policy = ResponseWritePolicy::connection(shutdown);
    let (mut parts, body) = response.into_parts();
    if !parts.headers.contains_key("content-length") {
        let length = tauri::http::HeaderValue::from_str(&body.len().to_string())
            .map_err(|error| error.to_string())?;
        parts.headers.insert("content-length", length);
    }
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\n",
        parts.status.as_u16(),
        parts.status.canonical_reason().unwrap_or("Media response")
    );
    for (name, value) in &parts.headers {
        if name == "connection" || name == "transfer-encoding" {
            continue;
        }
        let value = value
            .to_str()
            .map_err(|error| format!("invalid media response header: {error}"))?;
        headers.push_str(name.as_str());
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    write_response_bytes(stream, headers.as_bytes(), policy)?;
    write_response_bytes(stream, &body, policy)?;
    flush_response(stream, policy)
}

#[cfg(test)]
#[path = "lan_host_media_tests.rs"]
mod tests;
