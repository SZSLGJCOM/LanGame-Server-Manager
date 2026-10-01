use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn item(app_id: u64, description: &str) -> Value {
    json!({"id":app_id,"appid":app_id,"item_type":0,"success":1,"visible":true,
        "full_description_bbcode":description})
}

fn response(item: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"response":{"store_items":[item]}})).unwrap()
}

#[test]
fn regional_description_identifies_the_game_and_renders_full_copy() {
    let html = parse_description(
        &response(item(
            322330,
            "[h2]游戏介绍[/h2]完整正文 & <script>unsafe</script>",
        )),
        322330,
    )
    .unwrap()
    .unwrap();
    assert!(html.contains("游戏介绍"));
    assert!(html.contains("完整正文"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn regional_description_rejects_wrong_or_ambiguous_identity() {
    for payload in [
        response(item(252490, "Wrong game")),
        response(
            json!({"id":322330,"appid":322330,"item_type":1,"success":1,"visible":true,"full_description_bbcode":"Wrong type"}),
        ),
        serde_json::to_vec(
            &json!({"response":{"store_items":[item(322330,"First"),item(322330,"Second")]}}),
        )
        .unwrap(),
        response(
            json!({"id":322330,"success":1,"visible":true,"full_description_bbcode":"Missing identity"}),
        ),
        b"<html>Steam sign in</html>".to_vec(),
    ] {
        assert!(matches!(
            parse_description(&payload, 322330),
            Err(AboutError::Invalid(_))
        ));
    }
}

#[test]
fn unavailable_items_are_terminal_but_missing_copy_may_recover() {
    for (success, visible) in [(15, true), (1, false)] {
        let mut value = item(322330, "Restricted copy");
        value["success"] = json!(success);
        value["visible"] = json!(visible);
        let error = parse_description(&response(value), 322330).unwrap_err();
        assert!(!error.permits_fallback());
    }
    assert_eq!(
        parse_description(&response(item(322330, "  ")), 322330).unwrap(),
        None
    );
    let mut value = item(322330, "");
    value
        .as_object_mut()
        .unwrap()
        .remove("full_description_bbcode");
    assert_eq!(parse_description(&response(value), 322330).unwrap(), None);
}

#[test]
fn description_request_is_public_and_contains_only_the_selected_game_and_language() {
    for (locale, language, country) in [("zh-CN", "schinese", "CN"), ("en-US", "english", "US")] {
        let url = request_url(322330, Some(locale));
        let pairs = url.query_pairs().collect::<Vec<_>>();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "input_json");
        let input: Value = serde_json::from_str(&pairs[0].1).unwrap();
        assert_eq!(
            input,
            json!({"ids":[{"appid":322330}],"context":{"language":language,"country_code":country},"data_request":{"include_full_description":true}})
        );
        let candidates = app_network::official_url_candidates(
            url.as_str(),
            app_network::SourcePreference::from_locale(Some(locale)),
        );
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            reqwest::Url::parse(&candidates[0]).unwrap().host_str(),
            Some(if locale == "zh-CN" {
                "api.steamchina.com"
            } else {
                "api.steampowered.com"
            })
        );
    }
}

struct Fixture {
    url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(status: u16, body: Vec<u8>, stall: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/description", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            seen.fetch_add(1, Ordering::Relaxed);
            let size = if stall { body.len() + 1 } else { body.len() };
            socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            if stall {
                std::future::pending::<()>().await;
            }
        }
    });
    Fixture {
        url,
        requests,
        task,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test]
async fn wrong_identity_or_stalled_body_can_use_verified_alternate() {
    for stall in [false, true] {
        let primary = fixture(200, response(item(252490, "Wrong game")), stall).await;
        let alternate = fixture(
            200,
            response(item(322330, "Correct official full description")),
            false,
        )
        .await;
        let html = fetch_candidates(
            &client(),
            322330,
            &[primary.url.clone(), alternate.url.clone()],
            Duration::from_secs(1),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(html.contains("Correct official full description"));
        assert!(!html.contains("Wrong game"));
        assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn denied_or_throttled_source_never_contacts_an_alternate() {
    for status in [401, 403, 429] {
        let primary = fixture(status, Vec::new(), false).await;
        let alternate = fixture(200, response(item(322330, "Must not be fetched")), false).await;
        let error = fetch_candidates(
            &client(),
            322330,
            &[primary.url.clone(), alternate.url.clone()],
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(!error.permits_fallback());
        assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 0);
    }
}

#[tokio::test]
#[ignore = "Explicit read-only real official regional store description probe"]
async fn live_regional_full_descriptions_are_equivalent() {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("LanGame Server Manager official description verification")
        .build()
        .unwrap();
    let mut stories = Vec::new();
    for app_id in [322330, 1623730, 892970, 108600, 427520] {
        for locale in ["zh-CN", "en-US"] {
            let url = request_url(app_id, Some(locale));
            let candidates = app_network::official_url_candidates(
                url.as_str(),
                app_network::SourcePreference::from_locale(Some(locale)),
            );
            assert_eq!(candidates.len(), 2);
            let mut descriptions = Vec::new();
            for candidate in candidates {
                let html = fetch_candidates(
                    &client,
                    app_id,
                    std::slice::from_ref(&candidate),
                    Duration::from_secs(8),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(!html.trim().is_empty());
                println!(
                    "Official full description app={app_id} locale={locale} origin={} html_bytes={}",
                    reqwest::Url::parse(&candidate)
                        .unwrap()
                        .origin()
                        .ascii_serialization(),
                    html.len()
                );
                descriptions.push(html);
            }
            assert_eq!(
                descriptions[0], descriptions[1],
                "Same game and language must identify equivalent full descriptions"
            );
            stories.push(json!({"appId":app_id,"locale":locale,"html":descriptions[0]}));
        }
    }
    if let Some(path) = std::env::var_os("LANGAME_STORY_PROBE_FILE") {
        let path = std::path::PathBuf::from(path);
        assert!(
            path.is_absolute(),
            "Probe evidence must use an explicit absolute path"
        );
        let parent = path.parent().unwrap().canonicalize().unwrap();
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        assert!(
            !parent.starts_with(repository),
            "Public live response evidence stays outside the source tree"
        );
        let output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(
            output,
            &json!({"source":"production-regional-steam-api","stories":stories}),
        )
        .unwrap();
    }
}
