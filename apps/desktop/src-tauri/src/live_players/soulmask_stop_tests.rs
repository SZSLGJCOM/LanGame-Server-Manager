use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

const SHUTDOWN_HELP: &str = "  2: SaveAndExit\r\n\tAlia: close exit shutdown\r\n\tDESC: After the number of seconds specified by the parameter, save and exit the game world.\r\n\tExample: [close 300]\r\n\r\n  3: StopCloseServer\r\n";

fn untracked_details() -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary": {"id": "soul-stop-lease", "name": "Soul stop lease", "module_id": "soulmask",
            "status": app_core::InstanceStatus::Running, "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false},
        "config_file_path": "", "saves_path": "", "auto_backup_on_stop": false,
        "backup_retention_count": 1, "settings_json": "{}", "ports": [], "active_run": null
    }))
    .unwrap()
}

#[tokio::test]
async fn app_exit_reuses_its_shutdown_lease_without_reopening_ordinary_admission() {
    let state = crate::state::DesktopState::default();
    assert!(state.begin_app_shutdown(true).unwrap());
    let lease = state.begin_storage_shutdown_exclusive().unwrap();
    assert!(
        state
            .begin_storage_context_operation("late ordinary work")
            .is_err()
    );
    let error = save_and_exit(&lease, &untracked_details(), None)
        .await
        .unwrap_err();
    assert_eq!(error, "The current managed run is unavailable.");
    assert!(
        state
            .begin_storage_context_operation("still closed")
            .is_err()
    );
}

async fn read_help_query(peer: &mut BufReader<TcpStream>) -> String {
    let mut command = String::new();
    peer.read_line(&mut command).await.unwrap();
    assert_eq!(command, "help\r\n");
    let mut nonce = String::new();
    peer.read_line(&mut nonce).await.unwrap();
    assert!(nonce.starts_with("LGM_") && nonce.ends_with("\r\n"));
    nonce.trim_end().to_owned()
}

#[tokio::test]
async fn cancelling_while_help_is_pending_closes_the_socket_without_late_shutdown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (seen, received) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let nonce = read_help_query(&mut stream).await;
        seen.send(()).unwrap();
        released.await.unwrap();
        // Answering the previously pending help cannot revive a cancelled stop.
        let _ = stream
            .get_mut()
            .write_all(format!("{SHUTDOWN_HELP}{nonce} Not Found!\r\n").as_bytes())
            .await;
        let mut tail = Vec::new();
        let result = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut tail))
            .await
            .unwrap();
        if let Err(error) = result {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ));
        }
        assert!(
            tail.is_empty(),
            "a cancelled stop must never send SaveAndExit"
        );
    });
    let client = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address).await.unwrap();
        shutdown_exchange(&mut stream, async { Ok(()) }).await
    });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    client.abort();
    assert!(client.await.unwrap_err().is_cancelled());
    release.send(()).unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn native_shutdown_is_sent_once_only_after_help_and_ownership_validation() {
    for valid_owner in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let nonce = read_help_query(&mut stream).await;
            stream
                .get_mut()
                .write_all(format!("{SHUTDOWN_HELP}{nonce} Not Found!\r\n").as_bytes())
                .await
                .unwrap();
            let mut tail = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut tail))
                .await
                .unwrap()
                .unwrap();
            tail
        });
        let mut stream = TcpStream::connect(address).await.unwrap();
        let result = shutdown_exchange(&mut stream, async {
            if valid_owner {
                Ok(())
            } else {
                Err("fixture owner changed".into())
            }
        })
        .await;
        assert_eq!(result.is_ok(), valid_owner);
        drop(stream);
        let tail = peer.await.unwrap();
        assert_eq!(
            tail,
            if valid_owner {
                b"q\r\nSaveAndExit 1\r\n".as_slice()
            } else {
                b""
            }
        );
    }
}

struct PartialWriter {
    bytes: Vec<u8>,
    remaining: usize,
}

impl AsyncWrite for PartialWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.remaining == 0 {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
        }
        let count = input.len().min(self.remaining);
        self.bytes.extend_from_slice(&input[..count]);
        self.remaining -= count;
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn partial_shutdown_delivery_is_never_restarted_or_replayed() {
    let command = b"q\r\nSaveAndExit 1\r\n";
    for accepted in 0..=command.len() {
        let mut writer = PartialWriter {
            bytes: Vec::new(),
            remaining: accepted,
        };
        let result = send_save_and_exit(&mut writer).await;
        assert_eq!(writer.bytes, command[..accepted]);
        if accepted == command.len() {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(error.contains("may have been sent") && error.contains("not resent"));
        }
    }
}
