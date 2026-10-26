use super::*;

fn store_evidence(root: &Directory) -> serde_json::Value {
    let index = std::fs::read(root.0.join("index.json"))
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|error| error.to_string())
        });
    match index {
        Ok(index) => serde_json::json!({
            "blobs": root.blobs().len(),
            "entries": index["entries"].as_object().map(|entries| entries.values().map(|entry| serde_json::json!({
                "source": entry["source"], "headers": entry["headers"], "freshness": entry["freshness"],
                "hasFullBody": !entry["full"].is_null()
            })).collect::<Vec<_>>())
        }),
        Err(error) => serde_json::json!({ "indexError": error }),
    }
}

#[tokio::test]
#[ignore = "opt-in after publication: verifies both locale routes through the real ARK mirror cache"]
async fn ark_mirror_artwork_live_fetch_verifies_hash_and_reopens_each_locale() {
    const RESOURCE_ID: &str = "ark-map-asa-theisland_wp";
    let policy: serde_json::Value =
        serde_json::from_str(include_str!("../../official-sources.json")).unwrap();
    let resource = policy["exactResources"]
        .as_array()
        .expect("production exact resources")
        .iter()
        .find(|resource| resource["id"].as_str() == Some(RESOURCE_ID))
        .expect("publish and enable the reviewed ARK mirrors before running this opt-in test");
    let sources = resource["urls"]
        .as_array()
        .expect("reviewed mirror URLs")
        .iter()
        .map(|value| value.as_str().expect("mirror URL"))
        .collect::<Vec<_>>();
    assert_eq!(
        sources.len(),
        2,
        "this evidence checks the two published mirrors"
    );
    let expected_sha256 = resource["sha256"].as_str().expect("frozen image digest");
    assert_eq!(
        resource["cacheImmutable"].as_bool(),
        Some(true),
        "enable explicit immutable-resource persistence before this live test"
    );
    let mut failures = Vec::new();

    for (locale, preference) in [
        ("zh-CN", SourcePreference::ChinaFirst),
        ("en-US", SourcePreference::InternationalFirst),
    ] {
        // Each locale starts cold: the second route must not reuse the first
        // locale's successful disk entry instead of contacting its own source.
        let root = Directory::new();
        let cache = MediaCache::new(root.0.clone());
        let first = match cache
            .respond(sources[0], MediaKind::Image, preference, None, false)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("{locale}: cold download failed: {error}"));
                continue;
            }
        };
        if first.status != 200 {
            failures.push(format!("{locale}: cold status {}", first.status));
        }
        if header(&first, "X-LanGame-Media-Cache") != Some("miss") {
            failures.push(format!("{locale}: cold cache must report miss"));
        }
        if !sources.contains(&first.final_url.as_str()) {
            failures.push(format!("{locale}: unreviewed final source"));
        }
        let actual_sha256 = store::digest(&first.body);
        if actual_sha256 != expected_sha256 {
            failures.push(format!("{locale}: frozen digest mismatch"));
        }
        println!(
            "ARK_MIRROR_NATIVE locale={locale} source={} bytes={} sha256={actual_sha256}",
            first.final_url,
            first.body.len()
        );
        println!(
            "ARK_MIRROR_HEADERS locale={locale} headers={:?}",
            first.headers
        );
        println!(
            "ARK_MIRROR_STORE locale={locale} after=download {}",
            store_evidence(&root)
        );

        drop(cache);
        let reopened = MediaCache::new(root.0.clone());
        // Use the other URL to prove that different mirror paths share the same
        // verified resource identity after the cache owner has been dropped.
        let second = match reopened
            .respond(sources[1], MediaKind::Image, preference, None, false)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("{locale}: reopened cache failed: {error}"));
                continue;
            }
        };
        let cache_state = header(&second, "X-LanGame-Media-Cache");
        if cache_state != Some("hit") {
            failures.push(format!(
                "{locale}: reopened cache must hit, got {cache_state:?}"
            ));
        }
        if second.final_url != first.final_url {
            failures.push(format!("{locale}: reopened source changed"));
        }
        if second.body != first.body {
            failures.push(format!("{locale}: reopened bytes changed"));
        }
        println!(
            "ARK_MIRROR_REOPEN locale={locale} cache={cache_state:?} source={}",
            second.final_url
        );
        println!(
            "ARK_MIRROR_STORE locale={locale} after=reopen {}",
            store_evidence(&root)
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
