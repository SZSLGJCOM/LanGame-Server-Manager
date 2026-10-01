use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const PROTOCOL: u32 = 1;
pub(super) const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Request {
    pub protocol: u32,
    pub command: String,
    pub args: Value,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Response {
    pub result: Result<Value, String>,
}

pub(super) async fn read_frame<T: for<'a> Deserialize<'a>>(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<T, String> {
    let len = stream.read_u32_le().await.map_err(|e| e.to_string())? as usize;
    if len == 0 || len > MAX_FRAME {
        return Err("Runtime service frame exceeds the supported size".into());
    }
    let mut bytes = vec![0; len];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid runtime service frame: {e}"))
}

pub(super) async fn write_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> Result<(), String> {
    let bytes = encode_frame(value, MAX_FRAME)?;
    write_bytes(stream, &bytes).await
}

pub(super) async fn write_response(
    stream: &mut (impl AsyncWrite + Unpin),
    response: &Response,
) -> Result<(), String> {
    let bytes = encode_response(response, MAX_FRAME)?;
    write_bytes(stream, &bytes).await
}

fn encode_response(response: &Response, limit: usize) -> Result<Vec<u8>, String> {
    encode_frame(response, limit).or_else(|_| {
        encode_frame(&Response { result: Err(String::from(
            "Runtime result exceeds the response size limit; the operation may have completed. Inspect task state before retrying."
        )) }, MAX_FRAME)
    })
}

fn encode_frame(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, String> {
    struct BoundedFrame {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for BoundedFrame {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "Runtime service frame exceeds the supported size",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut frame = BoundedFrame {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut frame, value).map_err(|e| e.to_string())?;
    Ok(frame.bytes)
}

async fn write_bytes(stream: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> Result<(), String> {
    stream
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(bytes).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_result_returns_an_explicit_error_without_an_unbounded_encoded_copy() {
        let response = Response {
            result: Ok(Value::String("x".repeat(4096))),
        };
        assert!(encode_frame(&response, 256).unwrap_err().contains("size"));
        let bytes = encode_response(&response, 256).unwrap();
        let fallback: Response = serde_json::from_slice(&bytes).unwrap();
        assert!(fallback.result.unwrap_err().contains("may have completed"));
        assert!(bytes.len() < 256);
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_before_allocating_payload() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer.write_u32_le((MAX_FRAME + 1) as u32).await.unwrap();
        assert!(
            read_frame::<Request>(&mut reader)
                .await
                .unwrap_err()
                .contains("size")
        );
    }

    #[tokio::test]
    async fn framed_response_preserves_errors_and_unicode() {
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_frame(
            &mut writer,
            &Response {
                result: Err("实例启动失败".into()),
            },
        )
        .await
        .unwrap();
        let result: Response = read_frame(&mut reader).await.unwrap();
        assert_eq!(result.result, Err("实例启动失败".into()));
    }
}
