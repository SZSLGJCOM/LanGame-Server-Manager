use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

const CHILD_MARKER: &str = "LANGAME_LAUNCH_PROXY_CHILD";

pub(super) fn is_child() -> bool {
    std::env::var(CHILD_MARKER).as_deref() == Ok("1")
}

pub(super) async fn run_with_rejected_metadata() -> LaunchTestResult {
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, LAUNCH_MODULE)?;
    let manifest = descriptor
        .install
        .as_ref()
        .and_then(|install| install.minecraft.as_ref())
        .and_then(|minecraft| minecraft.manifest_url.as_deref())
        .ok_or("Minecraft manifest URL")?;
    let url = reqwest::Url::parse(manifest)?;
    assert_eq!(url.scheme(), "https");
    let expected_connect = format!(
        "CONNECT {}:{} HTTP/1.1",
        url.host_str().ok_or("manifest host")?,
        url.port_or_known_default().ok_or("manifest port")?
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let proxy = format!("http://{}", listener.local_addr()?);
    let test_name = std::thread::current()
        .name()
        .ok_or("named test thread")?
        .to_owned();
    // Proxy variables belong only to this exact child test. Other parallel
    // tests retain their network environment and never share this fixture.
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["--exact", &test_name, "--test-threads=1", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("HTTP_PROXY", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("ALL_PROXY", &proxy)
        .env("NO_PROXY", "localhost,127.0.0.1,::1")
        .kill_on_drop(true)
        .spawn()?;
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed = requests.clone();
    let mut server: tokio::task::JoinHandle<Result<(), String>> = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
            tokio::time::timeout(Duration::from_secs(3), async {
                let mut header = Vec::new();
                while !header.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let count = stream
                        .read(&mut buffer)
                        .await
                        .map_err(|error| error.to_string())?;
                    if count == 0 || header.len() + count > 8192 {
                        return Err("invalid proxy request header".to_owned());
                    }
                    header.extend_from_slice(&buffer[..count]);
                }
                let line = std::str::from_utf8(&header)
                    .map_err(|error| error.to_string())?
                    .lines()
                    .next()
                    .ok_or("proxy request line")?
                    .to_owned();
                {
                    let mut requests = observed.lock().unwrap();
                    if requests.len() >= 32 {
                        return Err(
                            "automatic update exceeded the bounded proxy request budget".into()
                        );
                    }
                    requests.push(line);
                }
                stream
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                stream.shutdown().await.map_err(|error| error.to_string())
            })
            .await
            .map_err(|_| "local proxy request did not settle".to_owned())??;
        }
    });
    let completed = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::select! {
            status = child.wait() => status.map_err(|error| error.to_string()),
            result = &mut server => Err(format!("metadata proxy ended before the child: {result:?}")),
        }
    }).await;
    let success = matches!(&completed, Ok(Ok(status)) if status.success());
    if !success && child.try_wait()?.is_none() {
        child.kill().await?;
        child.wait().await?;
    }
    if server.is_finished() && success {
        server.await??;
    } else if !server.is_finished() {
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .any(|line| line == &expected_connect),
        "the automatic update must request the declared metadata authority through the local proxy"
    );
    assert!(success, "launch child failed: {completed:?}");
    Ok(())
}
