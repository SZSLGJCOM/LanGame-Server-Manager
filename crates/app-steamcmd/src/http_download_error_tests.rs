use super::*;

#[derive(Debug)]
struct Diagnostic {
    message: String,
    source: Option<Box<Diagnostic>>,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

#[test]
fn transport_diagnostic_preserves_tls_cause_and_redacts_inner_urls() {
    let api_key = "synthetic-fixture";
    let error = Diagnostic {
        message: format!("error sending request to https://user:password@cdn.example.test/private-path?api_key={api_key}#outer-fragment"),
        source: Some(Box::new(Diagnostic {
            message: "TLS handshake failed".into(),
            source: Some(Box::new(Diagnostic {
                message: "invalid peer certificate: UnknownIssuer (HTTPS://user:pass@tls.example.test/cert-path?key=deep-key#deep-fragment)".into(),
                source: None,
            })),
        })),
    };
    let formatted = download_error(
        "https://user:fixture@download.example.test/runtime.exe?key=source-key#source-fragment",
        network_error_detail(&error),
    );
    let message = formatted.to_string();
    assert!(message.contains("TLS handshake failed: invalid peer certificate: UnknownIssuer"));
    assert!(message.contains("https://cdn.example.test"));
    assert!(message.contains("https://tls.example.test"));
    for secret in [
        "password",
        "private-path",
        api_key,
        "outer-fragment",
        "user",
        "pass",
        "cert-path",
        "deep-key",
        "deep-fragment",
        "fixture",
        "source-key",
        "source-fragment",
    ] {
        assert!(!message.contains(secret), "diagnostic exposed {secret}");
    }
}

#[test]
fn transport_diagnostic_bounds_external_chain_depth_and_messages() {
    let mut error = Diagnostic {
        message: "TLS cause".into(),
        source: None,
    };
    for _ in 0..20 {
        error = Diagnostic {
            message: format!("layer {}", "x".repeat(2000)),
            source: Some(Box::new(error)),
        };
    }
    let detail = network_error_detail(&error);
    // Adjacent identical layers collapse, while traversal still stops at eight.
    assert_eq!(detail.chars().count(), 1024);
    assert!(!detail.contains("TLS cause"));
    assert_eq!(
        redact_diagnostic_urls("proxy https://user:fixture@bad%host/private?key=token"),
        "proxy [redacted URL]"
    );
}

#[tokio::test]
async fn actual_reqwest_transport_source_survives_the_production_error_boundary() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://user:password@{address}/private?api_key=secret");
    let error = response(&fixture_client(), &url, deadline())
        .await
        .unwrap_err();
    let message = error.to_string();
    // reqwest's outer Display is just "error sending request". A real socket
    // source must now survive app-network and the download error conversion.
    assert!(message.contains("error sending request"), "{message}");
    assert!(
        message.contains("tcp connect error")
            || message.contains("connection refused")
            || message.contains("actively refused"),
        "{message}"
    );
    assert!(!message.contains("user"), "{message}");
    assert!(!message.contains("password"), "{message}");
    assert!(!message.contains("secret"), "{message}");
}
