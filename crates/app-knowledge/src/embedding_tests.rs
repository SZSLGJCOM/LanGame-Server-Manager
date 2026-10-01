use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::*;
use sha2::{Digest, Sha256};

fn fixture_tokenizer() -> Tokenizer {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": "1.0", "truncation": null, "padding": null,
        "added_tokens": [], "normalizer": null,
        "pre_tokenizer": { "type": "Whitespace" },
        "post_processor": null, "decoder": null,
        "model": { "type": "WordLevel", "unk_token": "[UNK]",
            "vocab": { "[PAD]": 0, "[UNK]": 1, "server": 2, "password": 3,
                "[CLS]": 4, "[SEP]": 5 } }
    }))
    .unwrap();
    let mut tokenizer = prepare_tokenizer(&bytes, 6).unwrap();
    tokenizer.with_post_processor(Some(
        tokenizers::processors::template::TemplateProcessing::builder()
            .try_single("[CLS] $A [SEP]")
            .unwrap()
            .special_tokens(vec![("[CLS]", 4), ("[SEP]", 5)])
            .build()
            .unwrap(),
    ));
    tokenizer
}

#[test]
fn cls_pooling_uses_only_first_token_and_normalizes() {
    let mut hidden = vec![100.0_f32; 2 * DIMENSIONS];
    hidden[..DIMENSIONS].fill(0.0);
    hidden[0] = 3.0;
    hidden[1] = 4.0;
    let vector = cls_pool(&[1, 2, DIMENSIONS as i64], &hidden, 2).unwrap();
    assert!((vector[0] - 0.6).abs() < 1e-6);
    assert!((vector[1] - 0.8).abs() < 1e-6);
    assert!(vector[2..].iter().all(|value| *value == 0.0));
    hidden[DIMENSIONS] = f32::NAN;
    assert!(cls_pool(&[1, 2, DIMENSIONS as i64], &hidden, 2).is_err());
    assert!(cls_pool(&[1, 1, DIMENSIONS as i64], &[0.0; DIMENSIONS], 1).is_err());
    assert!(cls_pool(&[1, 0, DIMENSIONS as i64], &[], 0).is_err());
    assert!(cls_pool(&[1, 1, 2], &[3.0, 4.0], 1).is_err());
}

#[test]
fn initialization_rejects_configuration_and_tokenizer_mismatches() {
    let mut config = serde_json::json!({
        "model_type": "modernbert", "vocab_size": 180000,
        "hidden_size": 384, "num_hidden_layers": 12, "num_attention_heads": 12,
        "intermediate_size": 1536, "max_position_embeddings": 32768,
        "cls_token_id": 179934, "sep_token_id": 179938
    });
    assert!(validate_config(&serde_json::to_vec(&config).unwrap()).is_ok());
    config["cls_token_id"] = 0.into();
    assert!(validate_config(&serde_json::to_vec(&config).unwrap()).is_err());
    assert!(validate_config(b"{}").is_err());
    let tokenizer = fixture_tokenizer().to_string(false).unwrap();
    assert!(prepare_tokenizer(tokenizer.as_bytes(), 5).is_err());
}

#[test]
fn retrieval_has_special_tokens_without_e5_prefixes_or_truncation() {
    let tokenizer = fixture_tokenizer();
    assert_eq!(
        encode_tokens(&tokenizer, "server").unwrap().get_ids(),
        &[4, 2, 5]
    );
    assert!(encode_tokens(&tokenizer, " ").is_err());
    assert!(encode_tokens(&tokenizer, &"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
    let encoding = encode_tokens(&tokenizer, &"server ".repeat(MAX_TOKENS + 1)).unwrap();
    assert!(encoding.len() > MAX_TOKENS);
    assert!(validate_encoding(&encoding).is_err());
    assert!(validate_encoding(&encode_tokens(&tokenizer, "server").unwrap()).is_ok());
    assert!(validate_batch(&vec!["server".into(); MAX_BATCH_SIZE + 1]).is_err());
    assert!(validate_batch(&["x".repeat(MAX_BATCH_BYTES + 1)]).is_err());
    assert!(validate_batch(&["server".into()]).is_ok());
}

#[test]
fn model_download_rejects_non_official_redirects_and_digest_mismatches() {
    for url in [
        "https://huggingface.co/a",
        "https://us.aws.cdn.hf.co/a?download=1",
        "https://cas-bridge.xethub.hf.co/a",
        "https://cdn-lfs-us-1.hf.co/a",
        "https://cdn-lfs-eu-1.hf.co/a",
        "https://transfer.xethub.hf.co/a",
        "https://transfer.xethub-eu.hf.co/a",
        "https://aws.cdn.hf.co/a",
        "https://us-east-1.aws.cdn.hf.co/a",
        "https://us-west-2.aws.cdn.hf.co/a",
        "https://eu-west-3.aws.cdn.hf.co/a",
        "https://ap-southeast-1.aws.cdn.hf.co/a",
        "https://us.gcp.cdn.hf.co/a",
        "https://us-east1.us.gcp.cdn.hf.co/a",
        "https://us-central1.us.gcp.cdn.hf.co/a",
        "https://us-west4.us.gcp.cdn.hf.co/a",
        "https://europe-west4.us.gcp.cdn.hf.co/a",
        "https://asia-southeast1.us.gcp.cdn.hf.co/a",
    ] {
        assert!(download::allowed_model_url(
            &reqwest::Url::parse(url).unwrap()
        ));
    }
    for url in [
        "http://huggingface.co/a",
        "https://huggingface.co.example.com/a",
        "https://evil.hf.co/a",
        "https://unknown.us.gcp.cdn.hf.co/a",
        "https://us.gcp.cdn.hf.co.example.com/a",
        "https://user@us.gcp.cdn.hf.co/a",
        "http://cdn-lfs-us-1.hf.co/a",
        "https://cdn-lfs-us-1.hf.co:444/a",
        "https://huggingface.cn/a",
        "https://huggingface.co:444/a",
        "https://user:pass@huggingface.co/a",
        "https://127.0.0.1/a",
        "https://example.com/?next=https://huggingface.co/a",
    ] {
        assert!(!download::allowed_model_url(
            &reqwest::Url::parse(url).unwrap()
        ));
    }
    let spec = download::ModelFile {
        name: "fixture",
        remote_name: "fixture",
        size: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };
    assert!(
        download::validate_digest(
            &spec,
            3,
            &Sha256::digest(b"abc")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
        .is_ok()
    );
    assert!(download::validate_digest(&spec, 2, spec.sha256).is_err());
    assert!(download::validate_digest(&spec, 3, &"0".repeat(64)).is_err());
    assert_eq!(
        download::FILES.iter().map(|file| file.size).sum::<u64>(),
        DOWNLOAD_BYTES
    );
}

async fn model_response_fixture(
    responses: Vec<&'static str>,
) -> (String, tokio::task::JoinHandle<usize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for response in &responses {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0_u8; 1024];
                let size = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
                assert!(request.len() <= 16 * 1024);
                if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /model "));
            assert!(request.contains("accept-encoding: identity\r\n"));
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        responses.len()
    });
    (url, server)
}

#[tokio::test]
async fn model_header_deferrals_cannot_be_restarted_as_body_failures() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for response in [
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nRetry-After: Wed, 21 Oct 2037 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (url, server) = model_response_fixture(vec![response]).await;
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            download::request_model_file(&client, &url, &AtomicBool::new(false)),
        )
        .await
        .unwrap();
        // The model's outer restart loop accepts only Network (body transport)
        // failures. A server restriction must remain terminal at this boundary.
        assert!(matches!(result, Err(KnowledgeError::Unavailable(_))));
        assert_eq!(server.await.unwrap(), 1);
    }
}

#[tokio::test]
async fn model_header_recovery_is_bounded_and_returns_the_actual_body() {
    const TRANSIENT: &str = "HTTP/1.1 503 Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc";
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let (url, server) = model_response_fixture(vec![TRANSIENT, OK]).await;
    let response = download::request_model_file(&client, &url, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"abc");
    assert_eq!(server.await.unwrap(), 2);

    let (url, server) = model_response_fixture(vec![TRANSIENT, TRANSIENT]).await;
    let result = download::request_model_file(&client, &url, &AtomicBool::new(false)).await;
    assert!(matches!(result, Err(KnowledgeError::Unavailable(_))));
    assert_eq!(server.await.unwrap(), 2);
}

struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("langame-embedding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn load_verifies_content_even_when_a_model_file_has_the_expected_length() {
    let dir = TempDirectory::new();
    let spec = download::ModelFile {
        name: "fixture",
        remote_name: "fixture",
        size: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };
    std::fs::write(dir.0.join("fixture"), b"abc").unwrap();
    assert_eq!(download::read_verified(&dir.0, &spec).unwrap(), b"abc");
    std::fs::write(dir.0.join("fixture"), b"xyz").unwrap();
    assert!(download::read_verified(&dir.0, &spec).is_err());
    assert!(!installed(&dir.0));
    assert!(Embedder::load(&dir.0).is_err());
}

#[tokio::test]
async fn cancelled_model_download_does_not_create_files_or_contact_the_network() {
    let dir = TempDirectory::new();
    let absent = dir.0.join("absent");
    let progress_called = AtomicBool::new(false);
    let result = ensure_model(&absent, &AtomicBool::new(true), &|_, _| {
        progress_called.store(true, Ordering::Relaxed);
    })
    .await;
    assert!(matches!(result, Err(KnowledgeError::Cancelled)));
    assert!(!absent.exists());
    assert!(!progress_called.load(Ordering::Relaxed));
}

// Independently chosen Chinese information needs and English, source-derived
// passages. These test inference and retrieval, not the separate web extractor.
// Sources read on 2026-09-28:
// https://learn.microsoft.com/en-us/minecraft/creator/documents/bedrockserver/server-properties
// https://docs.palworldgame.com/settings-and-operation/configuration/
const RETRIEVAL_PASSAGES: [&str; 10] = [
    "Minecraft Bedrock max-players sets the server's player capacity. Increasing the number of simultaneous players may affect performance.",
    "Minecraft Bedrock allow-list restricts connections to accounts recorded in allowlist.json. Enable this setting to require approved players.",
    "Minecraft Bedrock player-idle-timeout disconnects inactive players after the configured number of minutes. Zero permits indefinite inactivity.",
    "Minecraft Bedrock level-seed controls random world generation. An empty seed causes the game to generate a random value.",
    "Palworld bIsUseBackupSaveData turns on world save backups. The additional save copies increase storage I/O.",
    "Palworld AdminPassword is the credential for obtaining administrator rights. It controls privileged server administration.",
    "Palworld ServerPassword protects entry into the server. Players must provide this password when joining.",
    "Minecraft Bedrock difficulty selects peaceful, easy, normal, or hard gameplay for the world.",
    "Minecraft Bedrock texturepack-required makes connected clients use the world's designated texture pack.",
    "Palworld DayTimeSpeedRate adjusts how quickly daytime advances, while NightTimeSpeedRate controls nighttime progression.",
];

const RETRIEVAL_QUERIES: [(&str, usize); 8] = [
    ("怎样设置服务器最多允许多少人同时在线？", 0),
    ("我只想让白名单里的朋友进来，应该改哪个设置？", 1),
    ("玩家长时间挂机后会被自动踢出，怎么设置这个时间？", 2),
    ("创建世界时怎么指定地图生成的种子？", 3),
    ("如何让游戏世界定期保留备份存档？", 4),
    ("取得服务器管理员权限需要设置什么口令？", 5),
    ("我想要求玩家输入密码才能加入服务器。", 6),
    ("白天和夜晚流逝得太快，要调哪个参数？", 9),
];

#[tokio::test]
#[ignore = "Explicit real-weight acceptance; set LANGAME_EMBEDDING_MODEL_DIR outside the repository"]
async fn real_model_download_and_cross_language_retrieval() {
    let model_dir = PathBuf::from(
        std::env::var_os("LANGAME_EMBEDDING_MODEL_DIR")
            .expect("LANGAME_EMBEDDING_MODEL_DIR must name an isolated model cache"),
    );
    assert!(model_dir.is_absolute(), "Model cache must be absolute");
    std::fs::create_dir_all(&model_dir).unwrap();
    let model_dir = std::fs::canonicalize(model_dir).unwrap();
    let repository =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(
        !model_dir.starts_with(repository),
        "Real model weights must remain outside the repository"
    );
    if std::env::var("LANGAME_EMBEDDING_DOWNLOAD").as_deref() == Ok("1") {
        ensure_model(&model_dir, &AtomicBool::new(false), &|_, _| {})
            .await
            .unwrap();
    }
    let started = std::time::Instant::now();
    let model = Embedder::load(&model_dir).unwrap();
    let loaded = started.elapsed();
    let passages = model
        .encode_batch(
            &RETRIEVAL_PASSAGES
                .iter()
                .map(|text| (*text).into())
                .collect::<Vec<_>>(),
            &AtomicBool::new(false),
        )
        .unwrap();
    let mut first = 0;
    let mut top_three = 0;
    for (query, expected) in RETRIEVAL_QUERIES {
        let encoded = model.encode(query).unwrap();
        let mut ranked: Vec<_> = passages
            .iter()
            .enumerate()
            .map(|(index, passage)| {
                (
                    index,
                    encoded.iter().zip(passage).map(|(a, b)| a * b).sum::<f32>(),
                )
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let rank = ranked
            .iter()
            .position(|(index, _)| *index == expected)
            .unwrap()
            + 1;
        first += usize::from(rank == 1);
        top_three += usize::from(rank <= 3);
        eprintln!(
            "Cross-language retrieval: {query}; expected={expected}; rank={rank}; top3={:?}",
            &ranked[..3]
        );
    }
    eprintln!("Model={MODEL_ID}@{REVISION}; top1={first}/8; recall@3={top_three}/8");
    eprintln!(
        "Embedding acceptance: load={loaded:?}, total={:?}",
        started.elapsed()
    );
    // Gate fixed before observing this model's results: all information needs
    // must be available to a small RAG context, with most ranked first.
    assert_eq!(
        top_three,
        RETRIEVAL_QUERIES.len(),
        "Chinese queries must retrieve every relevant English passage in the top three"
    );
    assert!(
        first >= 6,
        "At least six of eight independent Chinese queries must rank their relevant English passage first"
    );
}
