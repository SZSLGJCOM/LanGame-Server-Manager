use super::super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lgsm-media-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(path)
    }
    pub fn blobs(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("blob-"))
            .map(|entry| entry.path())
            .collect()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[derive(Clone)]
pub(super) struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
}
pub(super) struct Wire {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub declared: Option<usize>,
    pub gate: Option<Arc<tokio::sync::Notify>>,
}
impl Wire {
    pub fn image() -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "image/png".into()),
                ("Cache-Control".into(), "max-age=10".into()),
                ("ETag".into(), "\"one\"".into()),
            ],
            body: b"fixture-image".to_vec(),
            declared: None,
            gate: None,
        }
    }
    pub fn video(request: &Request, data: &[u8], version: u64) -> Self {
        let etag = format!("\"version-{version}\"");
        let headers = vec![
            ("Content-Type".into(), "video/mp4".into()),
            ("Cache-Control".into(), "max-age=10".into()),
            ("ETag".into(), etag.clone()),
        ];
        if request.headers.get("if-none-match") == Some(&etag) {
            return Self {
                status: 304,
                headers,
                body: vec![],
                declared: None,
                gate: None,
            };
        }
        let mut reply = Self {
            status: 200,
            headers,
            body: data.to_vec(),
            declared: None,
            gate: None,
        };
        if request
            .headers
            .get("if-range")
            .is_some_and(|value| value != &etag)
        {
            return reply;
        }
        if let Some(range) = request.headers.get("range") {
            let (start, end) = range
                .strip_prefix("bytes=")
                .unwrap()
                .split_once('-')
                .unwrap();
            let start = start.parse::<usize>().unwrap();
            let end = end.parse::<usize>().unwrap().min(data.len() - 1);
            if start >= data.len() {
                reply.status = 416;
                reply.body.clear();
                reply
                    .headers
                    .push(("Content-Range".into(), format!("bytes */{}", data.len())));
            } else {
                reply.status = 206;
                reply.body = data[start..=end].to_vec();
                reply.headers.push((
                    "Content-Range".into(),
                    format!("bytes {start}-{end}/{}", data.len()),
                ));
            }
        }
        reply
    }
}

pub(super) struct Server {
    pub base: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    pub async fn new(handler: impl Fn(&Request) -> Wire + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let logged = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.unwrap();
                        assert!(connections.len() < 64);
                        let (handler, logged) = (Arc::clone(&handler), Arc::clone(&logged));
                        connections.spawn(async move {
                            let mut bytes = Vec::new();
                            while !bytes.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                                let mut part = [0; 1024];
                                let count = stream.read(&mut part).await.unwrap();
                                if count == 0 { return; }
                                bytes.extend_from_slice(&part[..count]);
                                assert!(bytes.len() < 16_384);
                            }
                            let text = String::from_utf8(bytes).unwrap();
                            let mut lines = text.lines();
                            let mut first = lines.next().unwrap().split_whitespace();
                            let request = Request { method: first.next().unwrap().into(), path: first.next().unwrap().into(),
                                headers: lines.filter_map(|line| line.split_once(':'))
                                    .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_string())).collect() };
                            logged.lock().unwrap().push(request.clone());
                            let wire = handler(&request);
                            let mut headers = format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\nContent-Length: {}\r\n",
                                wire.status, wire.declared.unwrap_or(wire.body.len()));
                            for (key, value) in wire.headers { headers.push_str(&format!("{key}: {value}\r\n")); }
                            headers.push_str("\r\n");
                            if stream.write_all(headers.as_bytes()).await.is_err() { return; }
                            if let Some(gate) = wire.gate { gate.notified().await; }
                            if request.method != "HEAD" { let _ = stream.write_all(&wire.body).await; }
                        });
                    },
                    result = connections.join_next(), if !connections.is_empty() => { result.unwrap().unwrap(); }
                }
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    pub async fn wait_for(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.count() < count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) fn resolver(bases: Vec<String>) -> Arc<Resolver> {
    Arc::new(move |input, _, preference| {
        let url = reqwest::Url::parse(input).ok()?;
        if !bases.contains(&url.origin().ascii_serialization()) {
            return None;
        }
        let suffix = format!(
            "{}{}",
            url.path(),
            url.query()
                .map(|value| format!("?{value}"))
                .unwrap_or_default()
        );
        let mut candidates = bases
            .iter()
            .map(|base| format!("{base}{suffix}"))
            .collect::<Vec<_>>();
        if preference == SourcePreference::ChinaFirst {
            candidates.reverse();
        }
        Some((suffix, candidates))
    })
}
pub(super) fn cache(
    root: &Directory,
    resolve: Arc<Resolver>,
    clock: &Arc<AtomicU64>,
) -> MediaCache {
    let clock = Arc::clone(clock);
    test_cache(
        root.0.clone(),
        resolve,
        Arc::new(move || clock.load(Ordering::Relaxed)),
        32 * 1024 * 1024,
        100,
    )
}
pub(super) fn header<'a>(response: &'a MediaResponse, key: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}
pub(super) async fn image(cache: &MediaCache, url: &str) -> Result<MediaResponse, MediaCacheError> {
    cache
        .respond(
            url,
            MediaKind::Image,
            SourcePreference::InternationalFirst,
            None,
            false,
        )
        .await
}
pub(super) async fn video(
    cache: &MediaCache,
    url: &str,
    range: &str,
) -> Result<MediaResponse, MediaCacheError> {
    cache
        .respond(
            url,
            MediaKind::Video,
            SourcePreference::InternationalFirst,
            Some(range),
            false,
        )
        .await
}
