use super::*;

const ABC_SHA512: &str = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

fn modrinth_file_metadata(hashes: serde_json::Value, size: u64) -> ModrinthVersionFile {
    serde_json::from_value(serde_json::json!({
        "url": "https://cdn.modrinth.com/example.jar",
        "filename": "example.jar",
        "size": size,
        "hashes": hashes,
        "primary": true
    }))
    .unwrap()
}

#[test]
fn modrinth_metadata_requires_size_and_hashes() {
    for metadata in [
        serde_json::json!({"hashes": {"sha512": ABC_SHA512}}),
        serde_json::json!({"size": 3}),
        serde_json::json!({"size": -1, "hashes": {"sha512": ABC_SHA512}}),
        serde_json::json!({"size": 3, "hashes": {"sha512": null, "sha1": ABC_SHA1}}),
    ] {
        let mut file = serde_json::json!({"url": "https://cdn.modrinth.com/example.jar", "filename": "example.jar"});
        file.as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        assert!(serde_json::from_value::<ModrinthVersionFile>(file).is_err());
    }
}

#[test]
fn modrinth_metadata_rejects_missing_and_malformed_supported_checksums() {
    for hashes in [
        serde_json::json!({}),
        serde_json::json!({"md5": "0".repeat(32)}),
        serde_json::json!({"sha1": "0".repeat(39)}),
        serde_json::json!({"sha1": "g".repeat(40)}),
        serde_json::json!({"sha512": "", "sha1": ABC_SHA1}),
        serde_json::json!({"sha512": "0".repeat(127), "sha1": ABC_SHA1}),
        serde_json::json!({"sha512": "g".repeat(128), "sha1": ABC_SHA1}),
        serde_json::json!({"sha512": "é".repeat(64), "sha1": ABC_SHA1}),
    ] {
        let file = modrinth_file_metadata(hashes, 3);
        assert!(ModDownloadIntegrity::from_modrinth_file(&file).is_err());
    }
}

async fn download_integrity_fixture(
    root: &Path,
    file: &ModrinthVersionFile,
) -> Result<PathBuf, String> {
    let (url, server) = serve_single_response(vec![
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
        b"1\r\na\r\n".to_vec(),
        b"2\r\nbc\r\n0\r\n\r\n".to_vec(),
    ]);
    let result = download_mod_site_file_with_limit(
        &download_test_client(),
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: Some(&file.filename),
            fallback_extension: "jar",
            integrity: Some(ModDownloadIntegrity::from_modrinth_file(file).unwrap()),
            max_bytes: 20,
            download_root: root,
        },
    )
    .await;
    server.join().unwrap();
    result
}

#[tokio::test]
async fn modrinth_verified_download_prefers_sha512_and_supports_sha1_fallback() {
    for hashes in [
        serde_json::json!({"sha512": ABC_SHA512.to_uppercase(), "sha1": "0".repeat(40)}),
        serde_json::json!({"sha1": ABC_SHA1}),
    ] {
        let root = download_test_root("verified");
        let file = modrinth_file_metadata(hashes, 3);
        let path = download_integrity_fixture(&root, &file).await.unwrap();
        assert_eq!(fs::read(path).unwrap(), b"abc");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn interrupted_modrinth_download_restarts_its_checksum_state() {
    let root = download_test_root("verified-restart");
    let file = modrinth_file_metadata(serde_json::json!({"sha512": ABC_SHA512}), 3);
    let (url, server) = serve_responses(vec![
        vec![b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\na".to_vec()],
        vec![b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec()],
    ]);
    let path = download_mod_site_file_with_limit(
        &download_test_client(),
        ModDownloadRequest {
            download_url: &url,
            package_name: "package",
            version: "1.0",
            preferred_filename: Some(&file.filename),
            fallback_extension: "jar",
            integrity: Some(ModDownloadIntegrity::from_modrinth_file(&file).unwrap()),
            max_bytes: 20,
            download_root: &root,
        },
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(fs::read(path).unwrap(), b"abc");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn modrinth_checksum_mismatch_never_publishes_a_file_or_downgrades() {
    for (hashes, algorithm) in [
        (
            serde_json::json!({"sha512": "0".repeat(128), "sha1": ABC_SHA1}),
            "SHA-512",
        ),
        (serde_json::json!({"sha1": "0".repeat(40)}), "SHA-1"),
    ] {
        let root = download_test_root("checksum-mismatch");
        let file = modrinth_file_metadata(hashes, 3);
        let error = download_integrity_fixture(&root, &file).await.unwrap_err();
        assert!(
            error.contains(&format!("{algorithm} checksum mismatch")),
            "{error}"
        );
        assert_download_root_empty(&root);
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn modrinth_size_mismatch_never_publishes_a_file() {
    for size in [2, 4] {
        let root = download_test_root("metadata-size-mismatch");
        let file = modrinth_file_metadata(serde_json::json!({"sha512": ABC_SHA512}), size);
        let error = download_integrity_fixture(&root, &file).await.unwrap_err();
        assert!(error.contains("size does not match metadata"), "{error}");
        assert_download_root_empty(&root);
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn modrinth_metadata_over_the_download_limit_is_rejected_before_requesting() {
    let root = download_test_root("metadata-size-limit");
    let file = modrinth_file_metadata(serde_json::json!({"sha512": ABC_SHA512}), 21);
    let error = download_mod_site_file_with_limit(
        &download_test_client(),
        ModDownloadRequest {
            download_url: "http://127.0.0.1:0/unused",
            package_name: "package",
            version: "1.0",
            preferred_filename: Some(&file.filename),
            fallback_extension: "jar",
            integrity: Some(ModDownloadIntegrity::from_modrinth_file(&file).unwrap()),
            max_bytes: 20,
            download_root: &root,
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("too large"), "{error}");
    assert_download_root_empty(&root);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn verified_modrinth_file_is_removed_when_commit_is_cancelled() {
    let root = download_test_root("verified-cancel");
    let path = root.join("package.jar");
    let (messages, receiver) = tokio::sync::mpsc::channel(1);
    let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
    let file = modrinth_file_metadata(serde_json::json!({"sha512": ABC_SHA512}), 3);
    let integrity = ModDownloadIntegrity::from_modrinth_file(&file).unwrap();
    let writer_path = path.clone();
    let writer = tokio::task::spawn_blocking(move || {
        write_downloaded_mod_file(writer_path, receiver, ready_sender, Some(integrity))
    });
    ready_receiver.await.unwrap().unwrap();
    messages
        .send(ModDownloadWriteMessage::Chunk(b"abc".to_vec()))
        .await
        .unwrap();
    let (prepared, prepared_receiver) = tokio::sync::oneshot::channel();
    let (commit, commit_receiver) = std::sync::mpsc::channel();
    messages
        .send(ModDownloadWriteMessage::Finish {
            prepared,
            commit: commit_receiver,
        })
        .await
        .unwrap();
    prepared_receiver.await.unwrap().unwrap();
    drop(commit);
    let error = wait_for_mod_download_writer(writer).await.unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
    assert_download_root_empty(&root);
    fs::remove_dir_all(root).unwrap();
}
