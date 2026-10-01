//! Opt-in loopback fixture for real browser Range/HLS playback through this cache.
//! The caller supplies an MP4 and stops the server after its browser assertions.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Fixture {
    base: String,
    root: PathBuf,
    cache: Mutex<MediaCache>,
    movie: Vec<u8>,
    native_movie: Vec<u8>,
    init_length: usize,
    requests: AtomicU64,
    bytes: AtomicU64,
    offline: AtomicBool,
    stop: AtomicBool,
}

fn create_cache(root: PathBuf, base: String) -> MediaCache {
    let allowed = Mutex::new(HashMap::<String, String>::new());
    test_cache(
        root,
        Arc::new(move |url, kind, _| {
            if let Some(identity) = allowed.lock().unwrap().get(url) {
                return Some((identity.clone(), vec![url.to_string()]));
            }
            let identity = media_cache_identity(url, kind)?;
            let parsed = reqwest::Url::parse(url).ok()?;
            let suffix = parsed.path().rsplit('/').next()?;
            let query = parsed
                .query()
                .map(|value| format!("?{value}"))
                .unwrap_or_default();
            let upstream = format!("{base}/upstream/{suffix}{query}");
            allowed
                .lock()
                .unwrap()
                .insert(upstream.clone(), identity.clone());
            Some((identity, vec![upstream]))
        }),
        Arc::new(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        }),
        64 * 1024 * 1024,
        100,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires LGSM_MEDIA_CACHE_BROWSER_FIXTURE_DIR and a separate browser driver"]
async fn browser_fixture() {
    let work = PathBuf::from(
        std::env::var("LGSM_MEDIA_CACHE_BROWSER_FIXTURE_DIR").expect("fixture directory"),
    );
    let movie = std::fs::read(work.join("cache-fixture.mp4")).expect("generated MP4 fixture");
    let native_movie =
        std::fs::read(work.join("native-fixture.mp4")).expect("progressive MP4 fixture");
    let info: serde_json::Value =
        serde_json::from_slice(&std::fs::read(work.join("cache-fixture.json")).unwrap()).unwrap();
    let init_length = info["initLength"].as_u64().unwrap() as usize;
    assert!(movie.len() > BLOCK_SIZE as usize * 2 && movie.len() < OBJECT_LIMIT);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let root = work.join(format!(
        "cache-browser-store-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixture = Arc::new(Fixture {
        cache: Mutex::new(create_cache(root.clone(), base.clone())),
        base: base.clone(),
        root,
        movie,
        native_movie,
        init_length,
        requests: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        offline: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });
    std::fs::write(work.join("cache-browser-endpoint.txt"), &base).unwrap();
    println!("MEDIA_CACHE_BROWSER_URL={base}");
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut connections = tokio::task::JoinSet::new();
    let slots = Arc::new(Semaphore::new(32));
    while !fixture.stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result.unwrap();
                let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else { continue; };
                let fixture = Arc::clone(&fixture);
                connections.spawn(async move {
                    let _permit = permit;
                    let _ = tokio::time::timeout(Duration::from_secs(30), serve(stream, fixture)).await;
                });
            },
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            _ = connections.join_next(), if !connections.is_empty() => {},
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    assert!(
        fixture.stop.load(Ordering::SeqCst),
        "browser driver did not finish before fixture deadline"
    );
    assert!(fixture.requests.load(Ordering::SeqCst) > 0);
}

async fn serve(mut stream: TcpStream, fixture: Arc<Fixture>) -> Result<(), std::io::Error> {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut buffer = [0_u8; 2048];
        let length = stream.read(&mut buffer).await?;
        if length == 0 || request.len() + length > 16384 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..length]);
    }
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let path = first.split_whitespace().nth(1).unwrap_or("/");
    let range = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("range"))
        .map(|(_, value)| value.trim().to_string());
    let url = reqwest::Url::parse(&format!("{}{path}", fixture.base)).unwrap();
    let result = if url.path().starts_with("/control/") {
        match url.path() {
            "/control/restart" => {
                *fixture.cache.lock().unwrap() =
                    create_cache(fixture.root.clone(), fixture.base.clone())
            }
            "/control/offline" => fixture.offline.store(true, Ordering::SeqCst),
            "/control/online" => fixture.offline.store(false, Ordering::SeqCst),
            "/control/stop" => fixture.stop.store(true, Ordering::SeqCst),
            _ => {}
        }
        fixture_response(200, "application/json", serde_json::to_vec(&serde_json::json!({
            "requests": fixture.requests.load(Ordering::SeqCst), "bytes": fixture.bytes.load(Ordering::SeqCst),
            "offline": fixture.offline.load(Ordering::SeqCst),
        })).unwrap())
    } else if url.path() == "/cache" {
        let args = url.query_pairs().collect::<HashMap<_, _>>();
        let source = args
            .get("url")
            .map(|value| value.as_ref())
            .unwrap_or_default();
        let kind = match args.get("kind").map(|value| value.as_ref()) {
            Some("video") => MediaKind::Video,
            Some("hls") => MediaKind::Hls,
            _ => MediaKind::Image,
        };
        let cache = fixture.cache.lock().unwrap().clone();
        match cache
            .respond(
                source,
                kind,
                SourcePreference::InternationalFirst,
                range.as_deref(),
                first.starts_with("HEAD "),
            )
            .await
        {
            Ok(mut response) => {
                response
                    .headers
                    .push(("X-LanGame-Media-Origin".into(), source.into()));
                response
            }
            Err(error) => {
                eprintln!("browser fixture cache error: {error}");
                fixture_response(
                    error.status_code(),
                    "text/plain",
                    error.to_string().into_bytes(),
                )
            }
        }
    } else if url.path().starts_with("/upstream/") {
        fixture.requests.fetch_add(1, Ordering::SeqCst);
        if fixture.offline.load(Ordering::SeqCst) {
            fixture_response(503, "text/plain", b"fixture offline".to_vec())
        } else {
            upstream(&fixture, url.path(), range.as_deref()).await
        }
    } else {
        fixture_response(404, "text/plain", b"missing fixture route".to_vec())
    };
    let mut header = format!(
        "HTTP/1.1 {} Fixture\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Expose-Headers: Content-Range, X-LanGame-Media-Origin, X-LanGame-Media-Cache\r\n",
        result.status
    );
    if !result
        .headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        header.push_str(&format!("Content-Length: {}\r\n", result.body.len()));
    }
    for (key, value) in result.headers {
        if key.eq_ignore_ascii_case("cache-control") && url.path() == "/cache" {
            continue;
        }
        header.push_str(&format!("{key}: {value}\r\n"));
    }
    if url.path() == "/cache" {
        header.push_str("Cache-Control: no-store\r\n");
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&result.body).await?;
    stream.shutdown().await
}

fn fixture_response(status: u16, content_type: &str, body: Vec<u8>) -> MediaResponse {
    MediaResponse {
        status,
        headers: vec![("Content-Type".into(), content_type.into())],
        body,
        final_url: String::new(),
    }
}

async fn upstream(fixture: &Fixture, path: &str, range: Option<&str>) -> MediaResponse {
    let (content_type, data) = if path.ends_with("/native.mp4") {
        ("video/mp4", fixture.native_movie.clone())
    } else if path.ends_with(".mp4") {
        ("video/mp4", fixture.movie.clone())
    } else if path.ends_with("/master.m3u8") {
        ("application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=9000000,CODECS=\"avc1.42001E\",RESOLUTION=640x360\nplaylist.m3u8\n".to_vec())
    } else if path.ends_with("/playlist.m3u8") {
        ("application/vnd.apple.mpegurl", format!("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:7\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"movie.mp4\",BYTERANGE=\"{}@0\"\n#EXTINF:6.5,\n#EXT-X-BYTERANGE:{}@{}\nmovie.mp4\n#EXT-X-ENDLIST\n", fixture.init_length, fixture.movie.len() - fixture.init_length, fixture.init_length).into_bytes())
    } else {
        ("image/svg+xml", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"540\"><rect width=\"960\" height=\"540\" fill=\"#334b52\"/><text x=\"48\" y=\"300\" font-size=\"36\" fill=\"white\">Persistent media cache fixture</text></svg>".to_vec())
    };
    let total = data.len();
    let mut response = if let Some((start, end)) = range
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.split_once('-'))
    {
        let start = start.parse::<usize>().unwrap_or(0);
        let end = end.parse::<usize>().unwrap_or(total - 1).min(total - 1);
        if start > end {
            return fixture_response(416, "text/plain", Vec::new());
        }
        // Observe first-frame readiness while later blocks are still in flight.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let mut response = fixture_response(206, content_type, data[start..=end].to_vec());
        response.headers.push((
            "Content-Range".into(),
            format!("bytes {start}-{end}/{total}"),
        ));
        response
    } else {
        fixture_response(200, content_type, data)
    };
    fixture
        .bytes
        .fetch_add(response.body.len() as u64, Ordering::SeqCst);
    response.headers.extend([
        ("ETag".into(), "\"fixture-v1\"".into()),
        ("Cache-Control".into(), "public, max-age=86400".into()),
        ("Accept-Ranges".into(), "bytes".into()),
    ]);
    response
}
