//! Strict request framing for the single-request LAN transport.

use std::collections::HashMap;

#[derive(Debug)]
pub(super) struct HttpRequestHead {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub buffered_body: Vec<u8>,
}

pub(super) fn parse_head(bytes: &[u8], body: &[u8]) -> Result<HttpRequestHead, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid HTTP header encoding")?;
    let mut lines = text.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if !is_token(method)
        || !path.starts_with('/')
        || path.bytes().any(|byte| byte <= b' ' || byte >= 127)
        || path.contains('#')
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
    {
        return Err("invalid HTTP request line".into());
    }
    let mut headers = HashMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("invalid HTTP header")?;
        if !is_token(name)
            || value
                .bytes()
                .any(|byte| (byte < b' ' && byte != b'\t') || byte == 127)
        {
            return Err("invalid HTTP header".into());
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim_matches([' ', '\t']).to_owned();
        // Never pick a last value for framing, authority or authentication.
        // This service has no list-valued request headers it needs to merge.
        if headers.insert(name, value).is_some() {
            return Err("duplicate HTTP header".into());
        }
    }
    // Chunked bodies are deliberately unsupported. Ignoring this field would
    // interpret the same authenticated bytes differently from an HTTP proxy.
    if headers.contains_key("transfer-encoding") {
        return Err("transfer-encoding is not supported".into());
    }
    if headers.contains_key("expect") {
        return Err("HTTP expectations are not supported".into());
    }
    Ok(HttpRequestHead {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        buffered_body: body.to_vec(),
    })
}

pub(super) fn content_length(request: &HttpRequestHead) -> Result<usize, String> {
    let Some(value) = request.headers.get("content-length") else {
        return Ok(0);
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid content-length header".into());
    }
    value
        .parse()
        .map_err(|_| "invalid content-length header".into())
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_request_and_buffered_bytes() {
        let head = parse_head(
            b"POST /__langame/api?mode=test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nX-LanGame-Token: fixture",
            b"{}",
        ).unwrap();
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/__langame/api?mode=test");
        assert_eq!(head.headers["x-langame-token"], "fixture");
        assert_eq!(content_length(&head).unwrap(), 2);
        assert_eq!(head.buffered_body, b"{}");
    }

    #[test]
    fn rejects_ambiguous_headers_and_unsupported_framing() {
        for fields in [
            "Content-Length: 2\r\ncontent-length: 9",
            "X-LanGame-Token: first\r\nx-langame-token: second",
            "Host: one\r\nHost: two",
            "Content-Length: 2\r\nTransfer-Encoding: chunked",
            "Transfer-Encoding: chunked",
            " Content-Length: 2",
            "Content-Length : 2",
            "Missing-Colon",
            "X-Header: line\r\n folded",
            "X-Header: invalid\nvalue",
            "X-Header: \0",
            "Expect: 100-continue",
        ] {
            assert!(parse_head(format!("POST / HTTP/1.1\r\n{fields}").as_bytes(), b"").is_err());
        }
    }

    #[test]
    fn rejects_malformed_request_lines_and_lengths() {
        for line in [
            "GET /",
            "GET / HTTP/2",
            "GET / HTTP/1.1 extra",
            "GET\t/ HTTP/1.1",
            "GET /#fragment HTTP/1.1",
        ] {
            assert!(parse_head(line.as_bytes(), b"").is_err());
        }
        for length in ["+2", "-1", "2, 2", "", "99999999999999999999999999999"] {
            let head = parse_head(
                format!("POST / HTTP/1.1\r\nContent-Length: {length}").as_bytes(),
                b"",
            )
            .unwrap();
            assert!(content_length(&head).is_err());
        }
        assert!(parse_head(b"GET / HTTP/1.1\r\nX-Header: \xff", b"").is_err());
    }
}
