use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn query_fixture(response: String) -> Result<Vec<String>, String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "request headers must be complete");
            request.extend_from_slice(&buffer[..count]);
            assert!(request.len() <= 8192, "request headers must be bounded");
        }
        assert!(request.starts_with(b"GET /api/tags HTTP/1.1\r\n"));
        if let Err(error) = stream.write_all(response.as_bytes()).await {
            // Oversized responses are rejected before the server finishes writing.
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
            ));
        }
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(list_ollama_models(Some(&endpoint)), server)
    })
    .await
    .expect("model query and fixture must finish within their shared deadline");
    result
}

fn json_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn ollama_models_trim_names_and_keep_first_occurrence_order() {
    let response = json_response(
        "200 OK",
        r#"{"models":[{"name":" small:latest "},{"name":""},{"name":"large:latest"},{"name":"small:latest"}]}"#,
    );
    assert_eq!(
        query_fixture(response).await.unwrap(),
        ["small:latest", "large:latest"]
    );
}

#[tokio::test]
async fn ollama_models_reject_malformed_success_responses() {
    let error = query_fixture(json_response("200 OK", r#"{"models":{}}"#))
        .await
        .unwrap_err();
    assert!(error.contains("failed to decode Ollama model list"));
}

#[tokio::test]
async fn ollama_models_decode_errors_do_not_echo_provider_values() {
    let credential = ["model", "query", "fixture"].join("-");
    for body in [
        serde_json::json!({"models": format!("token={credential}")}),
        serde_json::json!({"models": [{"name": {"path": "C:/fixture/private/model"}}]}),
    ] {
        let error = query_fixture(json_response("200 OK", &body.to_string()))
            .await
            .unwrap_err();
        assert!(error.contains("failed to decode Ollama model list: Data at line"));
        assert!(!error.contains(&credential));
        assert!(!error.contains("C:/fixture/private/model"));
    }
}

#[tokio::test]
async fn ollama_models_redact_provider_errors() {
    let credential = ["model", "query", "fixture"].join("-");
    let body = serde_json::json!({
        "error": format!("token={credential}"),
        "path": "C:/fixture/private/models.log"
    });
    let error = query_fixture(json_response("401 Unauthorized", &body.to_string()))
        .await
        .unwrap_err();
    assert!(error.contains("Ollama returned 401 Unauthorized"));
    assert!(error.contains(ASSISTANT_REDACTED_VALUE));
    assert!(!error.contains(&credential));
    assert!(!error.contains("C:/fixture/private/models.log"));
}

#[tokio::test]
async fn ollama_models_reject_oversized_declared_bodies_before_decoding() {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ASSISTANT_MAX_RESPONSE_BYTES + 1
    );
    let error = query_fixture(response).await.unwrap_err();
    assert!(error.contains("byte limit"));
}

#[tokio::test]
async fn ollama_models_bound_bodies_without_content_length_on_success_and_error() {
    let body = "x".repeat(ASSISTANT_MAX_RESPONSE_BYTES + 1);
    for status in ["200 OK", "500 Internal Server Error"] {
        let response = format!("HTTP/1.1 {status}\r\nConnection: close\r\n\r\n{body}");
        let error = query_fixture(response).await.unwrap_err();
        assert!(error.contains("byte limit"), "status: {status}");
    }
}
