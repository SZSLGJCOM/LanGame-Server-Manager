use super::*;

#[test]
fn media_socket_responses_preserve_partial_content_headers_and_head_lengths() {
    for head in [false, true] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut response = Response::new(if head { Vec::new() } else { vec![4, 5, 6] });
        *response.status_mut() = tauri::http::StatusCode::PARTIAL_CONTENT;
        response
            .headers_mut()
            .insert("content-length", tauri::http::HeaderValue::from_static("3"));
        response.headers_mut().insert(
            "content-range",
            tauri::http::HeaderValue::from_static("bytes 4-6/20"),
        );
        let check: ShutdownCheck = Arc::new(|| false);
        write_media_response(&mut server, response, &check).unwrap();
        drop(server);
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).unwrap();
        let separator = bytes
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = std::str::from_utf8(&bytes[..separator]).unwrap();
        assert!(headers.starts_with("HTTP/1.1 206 Partial Content"));
        assert!(headers.contains("content-length: 3\r\n"));
        assert!(headers.contains("content-range: bytes 4-6/20\r\n"));
        assert_eq!(
            &bytes[separator..],
            if head { &[][..] } else { &[4, 5, 6][..] }
        );
    }
}

#[test]
fn media_response_write_stops_when_the_application_is_closing() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    let shutdown: ShutdownCheck = Arc::new(|| true);
    let error =
        write_media_response(&mut server, error_response(200, "fixture"), &shutdown).unwrap_err();
    assert!(error.contains("shutting down"));
}
