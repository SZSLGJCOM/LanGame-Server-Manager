use super::assistant_repair_integration_tests::read_repair_model_request;
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

pub(super) fn openai_tool_response(id: &str, name: &str, arguments: Value) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,"tool_calls":[{
            "id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}
        }]
    }}]})
}

pub(super) fn assert_native_tool_available(request: &Value, name: &str) {
    assert!(
        request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["type"] == "function" && tool["function"]["name"] == name }),
        "Expected the current request to advertise {name}"
    );
}

/// The caller owns the listener and surrounding scenario deadline. No background
/// task is spawned; cancellation drops the current socket and returns ownership.
pub(super) async fn serve_requirements_draft(
    listener: &TcpListener,
    requirements: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    for (id, name, arguments) in [
        (
            "requirements-record",
            "record_task_requirements",
            requirements,
        ),
        ("requirements-finish", "finish_task_requirements", json!({})),
    ] {
        tokio::time::timeout(Duration::from_secs(10), async {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            assert_native_tool_available(&request, name);
            assert!(request["tools"].as_array().unwrap().iter().all(|tool| {
                tool["function"]["name"] != "propose_operation"
            }), "a draft must finish before an operation can be proposed");
            if name == "finish_task_requirements" {
                let result = request["messages"].as_array().unwrap().iter().find(|message| {
                    message["role"] == "tool" && message["tool_call_id"] == "requirements-record"
                }).expect("record result must retain its original native call ID");
                let result: Value = serde_json::from_str(result["content"].as_str().unwrap())?;
                assert_eq!(result["ok"], true);
                assert_eq!(result["data"]["errors"], json!([]));
            }
            let body = openai_tool_response(id, name, arguments).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        }).await??;
    }
    Ok(())
}
