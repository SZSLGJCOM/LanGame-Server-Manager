//! Real updater/reqwest/minisign integration; only the HTTP origin is a fixture.
//! The disposable signing key was discarded. These public vectors sign inert
//! bytes, never an executable, and no test calls the installer or runtime service.
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tauri::{Url, test::MockRuntime};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::{JoinHandle, JoinSet},
};

use super::{AppUpdateError, AppUpdateInstallEvent, check_update_sources, download};

const VERSION: &str = "99.0.0";
const OFFICIAL: &str = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v99.0.0/LanGame.Server.Manager_99.0.0_x64-setup.exe";
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGRpc3Bvc2FibGUgTEdTTSB0cmFuc3BvcnQgdGVzdCBrZXkKUldRQkFnTUVCUVlIQ0paK3pHbzhieUoxNXZmTlpFWmd5QXJaM0RPb1hEOWcrOWsyRVZsQmdHQncK";
const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGRpc3Bvc2FibGUgTEdTTSB0cmFuc3BvcnQgdGVzdCBzaWduYXR1cmUKUlVRQkFnTUVCUVlIQ0xIRXlSUmo2VlFnL0FTOEp4Ukh3OUFRemdaMnZzRmY4SjhKNkxpbkJBNGN6cEEzMW4xemNPaGZhdXk5bk5Idjl1YUVSY0VZQ0JNQ0UybENNdG4wNXdjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDE3NjAwCWZpbGU6TGFuR2FtZSBTZXJ2ZXIgTWFuYWdlcl85OS4wLjBfeDY0LXNldHVwLmV4ZQl2ZXJzaW9uOjk5LjAuMApVcGFrRWtibWRTcU11bXd5MEsxdVZGVjE0c0NzNjVZMzBlTlFIK3laS0hnbWIxZm4zdXp4KzdDZGhLWmMzSnhuOEFXQ1RHZzhRVDVrQk02M3huZzhDZz09Cg==";
const MISSING_VERSION: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGRpc3Bvc2FibGUgTEdTTSB0cmFuc3BvcnQgdGVzdCBzaWduYXR1cmUKUlVRQkFnTUVCUVlIQ0xIRXlSUmo2VlFnL0FTOEp4Ukh3OUFRemdaMnZzRmY4SjhKNkxpbkJBNGN6cEEzMW4xemNPaGZhdXk5bk5Idjl1YUVSY0VZQ0JNQ0UybENNdG4wNXdjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDE3NjAwCWZpbGU6TGFuR2FtZSBTZXJ2ZXIgTWFuYWdlcl85OS4wLjBfeDY0LXNldHVwLmV4ZQpLaGFRNUNJVEIydkd1eFRQYUg1R0RIaitaZEs4UExmRlQrZzZtajJ2S1FkaUVpNnZpZHZMM2k0Mjlhb0lXbVVDeEZrV3lsaWthcFI1aWVnZDdYbDJCQT09Cg==";
const WRONG_VERSION: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGRpc3Bvc2FibGUgTEdTTSB0cmFuc3BvcnQgdGVzdCBzaWduYXR1cmUKUlVRQkFnTUVCUVlIQ0xIRXlSUmo2VlFnL0FTOEp4Ukh3OUFRemdaMnZzRmY4SjhKNkxpbkJBNGN6cEEzMW4xemNPaGZhdXk5bk5Idjl1YUVSY0VZQ0JNQ0UybENNdG4wNXdjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDE3NjAwCWZpbGU6TGFuR2FtZSBTZXJ2ZXIgTWFuYWdlcl85OS4wLjBfeDY0LXNldHVwLmV4ZQl2ZXJzaW9uOjk4LjAuMApZNTZNWUVzVWoyS2IvcU5Sd2pkc2JOdjhwMkw5aU9rOVBCVkYzYzRPNDVyN20zU2l4bE1Xd0dqa25zWFVhNjFzOHY2L21jcWVMRDVOYXFBMk94RXFEZz09Cg==";
const WINDOW: Duration = Duration::from_millis(500);

fn payload() -> Vec<u8> {
    b"LGSM updater transport fixture\n".repeat(1024)
}

fn manifest(url: &str, signature: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": VERSION,
        "platforms": { "windows-x86_64": { "url": url, "signature": signature } }
    }))
    .unwrap()
}

fn app() -> tauri::App<MockRuntime> {
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": PUBLIC_KEY,
            "requireSignedVersion": true,
            // Only this fixture permits loopback HTTP; production stays HTTPS.
            "dangerousInsecureTransportProtocol": true
        }),
    );
    tauri::test::mock_builder()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .target("windows-x86_64")
                .build(),
        )
        .build(context)
        .unwrap()
}

#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    NoLengthBody,
    NoLengthStalled,
    NoContent,
    Interrupted,
    Stalled,
    Slow,
}

struct HttpFixture {
    origin: Url,
    requests: Arc<Mutex<Vec<String>>>,
    cancelled: Arc<AtomicUsize>,
    cancellation: Arc<Notify>,
    server: JoinHandle<()>,
}

impl HttpFixture {
    async fn start(routes: &[(&str, Reply)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let routes: HashMap<_, _> = routes
            .iter()
            .map(|(path, reply)| ((*path).to_owned(), reply.clone()))
            .collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let cancellation = Arc::new(Notify::new());
        let (seen, count, signal) = (requests.clone(), cancelled.clone(), cancellation.clone());
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (routes, seen, count, signal) = (routes.clone(), seen.clone(), count.clone(), signal.clone());
                        connections.spawn(async move {
                            let result = serve(stream, &routes, seen, count, signal).await;
                            if let Err(error) = result {
                                assert!(matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted), "fixture HTTP failure: {error}");
                            }
                        });
                    }
                    completed = connections.join_next(), if !connections.is_empty() => {
                        completed.unwrap().expect("fixture connection task panicked");
                    }
                }
            }
        });
        Self {
            origin,
            requests,
            cancelled,
            cancellation,
            server,
        }
    }

    fn url(&self, path: &str) -> Url {
        self.origin.join(path).unwrap()
    }

    fn paths(&self) -> Vec<String> {
        assert!(
            !self.server.is_finished(),
            "fixture server unexpectedly stopped"
        );
        self.requests.lock().unwrap().clone()
    }

    async fn expect_cancelled(&self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notified = self.cancellation.notified();
                if self.cancelled.load(Ordering::SeqCst) > 0 {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("abandoned HTTP connection must close");
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        // Aborting the server drops its JoinSet, cancelling every child socket.
        self.server.abort();
    }
}

async fn serve(
    mut stream: TcpStream,
    routes: &HashMap<String, Reply>,
    requests: Arc<Mutex<Vec<String>>>,
    cancelled: Arc<AtomicUsize>,
    cancellation: Arc<Notify>,
) -> io::Result<()> {
    let mut request = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await?;
            if count == 0 || request.len() + count > 8192 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            request.extend_from_slice(&buffer[..count]);
        }
        Ok::<(), io::Error>(())
    })
    .await
    .map_err(|_| io::ErrorKind::TimedOut)??;
    let request = String::from_utf8(request).map_err(|_| io::ErrorKind::InvalidData)?;
    let path = request
        .split_whitespace()
        .nth(1)
        .ok_or(io::ErrorKind::InvalidData)?;
    requests.lock().unwrap().push(path.to_owned());
    let reply = routes.get(path).expect("unexpected fixture request");
    if matches!(reply, Reply::NoContent) {
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }
    let bytes = match reply {
        Reply::Body(bytes) => bytes.clone(),
        _ => payload(),
    };
    let length = if matches!(reply, Reply::NoLengthBody | Reply::NoLengthStalled) {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", bytes.len())
    };
    stream
        .write_all(format!("HTTP/1.1 200 OK\r\n{length}Connection: close\r\n\r\n").as_bytes())
        .await?;
    match reply {
        Reply::Body(_) | Reply::NoLengthBody => stream.write_all(&bytes).await?,
        Reply::Interrupted => stream.write_all(&bytes[..1024]).await?,
        Reply::Stalled | Reply::NoLengthStalled => {
            let prefix = if matches!(reply, Reply::NoLengthStalled) {
                2048
            } else {
                1024
            };
            stream.write_all(&bytes[..prefix]).await?;
            let mut byte = [0; 1];
            assert_eq!(
                stream.read(&mut byte).await?,
                0,
                "no second request expected"
            );
            cancelled.fetch_add(1, Ordering::SeqCst);
            cancellation.notify_one();
        }
        Reply::Slow => {
            for chunk in bytes.chunks(1024) {
                stream.write_all(chunk).await?;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        Reply::NoContent => unreachable!("handled before writing the body"),
    }
    Ok(())
}

async fn checked_update(app: &tauri::App<MockRuntime>, fixture: &HttpFixture) -> Update {
    check_update_sources(
        || app.updater_builder().no_proxy(),
        &[fixture.url("manifest")],
        Duration::from_secs(3),
    )
    .await
    .unwrap()
    .expect("fixture must announce a newer update")
}

async fn fetch(
    update: &Update,
    urls: &[Url],
    minimums: &[u64],
) -> (Result<Vec<u8>, AppUpdateError>, Vec<AppUpdateInstallEvent>) {
    let events = Mutex::new(Vec::new());
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        download::download_from_sources(
            update,
            urls,
            |event| events.lock().unwrap().push(event),
            WINDOW,
            minimums,
            256 * 1024 * 1024,
        ),
    )
    .await
    .expect("fixture download must stay bounded");
    (result, events.into_inner().unwrap())
}

fn attempts(events: &[AppUpdateInstallEvent]) -> Vec<usize> {
    let mut totals = Vec::new();
    for event in events {
        match event {
            AppUpdateInstallEvent::Started {
                content_length: None,
            } => totals.push(0),
            AppUpdateInstallEvent::Started {
                content_length: Some(length),
            } => assert_eq!(*length as usize, payload().len()),
            AppUpdateInstallEvent::Progress { chunk_length } => {
                *totals.last_mut().expect("progress needs Started") += chunk_length
            }
            AppUpdateInstallEvent::Finished => {}
            AppUpdateInstallEvent::Installing => panic!("download must not initiate installation"),
        }
    }
    totals
}

fn assert_finished(events: &[AppUpdateInstallEvent], expected: usize) {
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AppUpdateInstallEvent::Installing)),
        "transport must not start installation"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AppUpdateInstallEvent::Finished))
            .count(),
        expected
    );
    if expected > 0 {
        assert!(matches!(
            events.last(),
            Some(AppUpdateInstallEvent::Finished)
        ));
    }
}

#[tokio::test]
async fn malformed_or_unusable_primary_manifest_does_not_block_healthy_metadata() {
    let app = app();
    for bad in [
        b"<html>not a manifest</html>".to_vec(),
        b"{\"version\":\"99.0.0\"}".to_vec(),
        manifest("https://example.invalid/update.exe", SIGNATURE),
    ] {
        let fixture = HttpFixture::start(&[
            ("/bad", Reply::Body(bad)),
            ("/good", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
        ])
        .await;
        let update = check_update_sources(
            || app.updater_builder().no_proxy(),
            &[fixture.url("bad"), fixture.url("good")],
            Duration::from_secs(3),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(update.version, VERSION);
        assert_eq!(update.download_url.as_str(), OFFICIAL);
        assert_eq!(fixture.paths(), ["/bad", "/good"]);
    }
}

#[tokio::test]
async fn a_current_first_source_does_not_hide_a_newer_release_on_the_next_source() {
    let app = app();
    let mut current: serde_json::Value =
        serde_json::from_slice(&manifest(OFFICIAL, SIGNATURE)).unwrap();
    current["version"] = serde_json::Value::String(app.package_info().version.to_string());
    for response in [
        Reply::NoContent,
        Reply::Body(serde_json::to_vec(&current).unwrap()),
    ] {
        let fixture = HttpFixture::start(&[
            ("/current", response),
            ("/newer", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
        ])
        .await;
        let update = check_update_sources(
            || app.updater_builder().no_proxy(),
            &[fixture.url("current"), fixture.url("newer")],
            Duration::from_secs(3),
        )
        .await
        .unwrap()
        .expect("the second source has a newer release");
        assert_eq!(update.version, VERSION);
        assert_eq!(fixture.paths(), ["/current", "/newer"]);
    }
}

#[tokio::test]
async fn a_valid_current_response_survives_later_broken_metadata() {
    let app = app();
    let fixture = HttpFixture::start(&[
        ("/current", Reply::NoContent),
        ("/broken", Reply::Body(b"not JSON".to_vec())),
    ])
    .await;
    let update = check_update_sources(
        || app.updater_builder().no_proxy(),
        &[fixture.url("current"), fixture.url("broken")],
        Duration::from_secs(3),
    )
    .await
    .unwrap();
    assert!(update.is_none());
    assert_eq!(fixture.paths(), ["/current", "/broken"]);
}

#[tokio::test]
async fn a_fully_received_tampered_package_is_not_reported_as_finished() {
    let app = app();
    let mut tampered = payload();
    tampered[0] ^= 1;
    let fixture = HttpFixture::start(&[
        ("/manifest", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
        ("/tampered", Reply::Body(tampered)),
    ])
    .await;
    let update = checked_update(&app, &fixture).await;
    let (result, events) = fetch(&update, &[fixture.url("tampered")], &[1]).await;
    assert!(matches!(
        result,
        Err(AppUpdateError::Updater(
            tauri_plugin_updater::Error::Minisign(_)
        ))
    ));
    assert_eq!(attempts(&events), [payload().len()]);
    assert_finished(&events, 0);
}

#[tokio::test]
async fn interrupted_and_tampered_artifacts_fall_back_with_fresh_progress() {
    let app = app();
    let mut tampered = payload();
    tampered[0] ^= 1;
    for bad in [Reply::Interrupted, Reply::Body(tampered)] {
        let fixture = HttpFixture::start(&[
            ("/manifest", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
            ("/bad", bad),
            ("/good", Reply::Body(payload())),
        ])
        .await;
        let update = checked_update(&app, &fixture).await;
        let (result, events) = fetch(
            &update,
            &[fixture.url("bad"), fixture.url("good")],
            &[16 * 1024, 1],
        )
        .await;
        assert_eq!(result.unwrap(), payload());
        let totals = attempts(&events);
        assert_eq!(totals.len(), 2);
        assert!(totals[0] > 0);
        assert_eq!(
            totals[1],
            payload().len(),
            "retry progress must not include abandoned bytes"
        );
        assert_finished(&events, 1);
        assert_eq!(fixture.paths(), ["/manifest", "/bad", "/good"]);
    }
}

#[tokio::test]
async fn invalid_missing_or_mismatched_signed_version_never_finishes_download() {
    let app = app();
    for signature in ["invalid signature", MISSING_VERSION, WRONG_VERSION] {
        let fixture = HttpFixture::start(&[
            ("/manifest", Reply::Body(manifest(OFFICIAL, signature))),
            ("/artifact", Reply::Body(payload())),
        ])
        .await;
        let update = checked_update(&app, &fixture).await;
        let (result, events) = fetch(&update, &[fixture.url("artifact")], &[1]).await;
        let error = result.expect_err("unverified bytes must not reach the install boundary");
        match signature {
            MISSING_VERSION => assert!(matches!(
                error,
                AppUpdateError::Updater(tauri_plugin_updater::Error::MissingSignedVersion)
            )),
            WRONG_VERSION => assert!(matches!(
                error,
                AppUpdateError::Updater(tauri_plugin_updater::Error::SignedVersionMismatch { .. })
            )),
            _ => assert!(matches!(error, AppUpdateError::Updater(_))),
        }
        assert_eq!(attempts(&events), [payload().len()]);
        assert_finished(&events, 0);
    }
}

#[tokio::test]
async fn watchdog_abandons_partial_connection_and_resets_the_next_source() {
    let app = app();
    let fixture = HttpFixture::start(&[
        ("/manifest", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
        ("/stall", Reply::Stalled),
        ("/good", Reply::Body(payload())),
    ])
    .await;
    let update = checked_update(&app, &fixture).await;
    let (result, events) = fetch(
        &update,
        &[fixture.url("stall"), fixture.url("good")],
        &[16 * 1024, 1],
    )
    .await;
    assert_eq!(result.unwrap(), payload());
    assert_eq!(attempts(&events), [1024, payload().len()]);
    assert_finished(&events, 1);
    fixture.expect_cancelled().await;
    assert_eq!(fixture.paths(), ["/manifest", "/stall", "/good"]);
}

#[tokio::test]
async fn second_pass_accepts_a_slow_source_that_keeps_transferring() {
    let app = app();
    let fixture = HttpFixture::start(&[
        ("/manifest", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
        ("/slow", Reply::Slow),
    ])
    .await;
    let update = checked_update(&app, &fixture).await;
    let (result, events) = fetch(&update, &[fixture.url("slow")], &[16 * 1024, 1]).await;
    assert_eq!(result.unwrap(), payload());
    let totals = attempts(&events);
    assert_eq!(totals.len(), 2);
    assert!(totals[0] > 0 && totals[0] < 16 * 1024);
    assert_eq!(totals[1], payload().len());
    assert_finished(&events, 1);
    assert_eq!(fixture.paths(), ["/manifest", "/slow", "/slow"]);
}

#[tokio::test]
async fn oversized_announced_streamed_and_completed_bodies_never_finish() {
    let app = app();
    for reply in [Reply::Stalled, Reply::NoLengthStalled, Reply::NoLengthBody] {
        let stalled = !matches!(reply, Reply::NoLengthBody);
        let fixture = HttpFixture::start(&[
            ("/manifest", Reply::Body(manifest(OFFICIAL, SIGNATURE))),
            ("/oversized", reply),
        ])
        .await;
        let update = checked_update(&app, &fixture).await;
        let events = Mutex::new(Vec::new());
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            download::download_from_sources(
                &update,
                &[fixture.url("oversized")],
                |event| events.lock().unwrap().push(event),
                WINDOW,
                &[1],
                1024,
            ),
        )
        .await
        .expect("oversized download must be cancelled promptly");
        assert!(
            matches!(result, Err(AppUpdateError::DownloadTooLarge)),
            "{result:?}"
        );
        assert_finished(&events.into_inner().unwrap(), 0);
        if stalled {
            fixture.expect_cancelled().await;
        }
        assert_eq!(fixture.paths(), ["/manifest", "/oversized"]);
    }
}
