use crate::{KnowledgeLibrary, Result};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};

/// Exercise recovered publisher channels through the same learned retrieval
/// and paged reads that LAN uses, including citations after reopening the cache.
#[tokio::test]
#[ignore = "requires LANGAME_KNOWLEDGE_LIVE_ROOT after syncing the recovered game sources"]
async fn real_recovered_game_corpus_retrieval() -> Result<()> {
    let root = PathBuf::from(std::env::var("LANGAME_KNOWLEDGE_LIVE_ROOT").unwrap());
    assert!(root.is_absolute());
    let modules = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let library = KnowledgeLibrary::open(&root, &modules).await?;
    let mut reports = Vec::new();
    let mut reads = Vec::new();
    for (module, query, terms, citation_prefix) in [
        (
            "corekeeper",
            "dedicated server direct connections IP Port Password",
            ["direct connections", "IP, Port and Password"],
            "https://store.steampowered.com/news/posts/",
        ),
        (
            "enshrouded",
            "dedicated server configuration queryPort default port",
            ["queryPort", "15637"],
            "https://enshrouded.zendesk.com/hc/en-us/articles/",
        ),
        (
            "runescapedragonwilds",
            "world management latest .sav save backup",
            [".sav", "RSDragonwilds/Saved/Savegames"],
            "https://dragonwilds.runescape.com/news/how-to-dedicated-servers",
        ),
        (
            "soulmask",
            "save progress before restart gm BaoCun",
            ["gm", "BaoCun"],
            "https://store.steampowered.com/news/posts/",
        ),
    ] {
        let page = library.search(module, query, 0).await?;
        let evidence = page
            .entries
            .iter()
            .map(|entry| entry.snippet.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let found = terms.iter().all(|term| evidence.contains(term));
        let cited = page.entries.iter().any(|entry| {
            entry.source.kind == "official" && entry.source.url.starts_with(citation_prefix)
        });
        for entry in &page.entries {
            if entry.source.content_use == crate::ContentUse::Full {
                reads.push((
                    module,
                    entry.id.clone(),
                    entry.offset_bytes,
                    entry.snippet.clone(),
                    entry.source.url.clone(),
                    entry.source.content_sha256.clone(),
                ));
            }
        }
        reports.push(serde_json::json!({"module":module,"query":query,
            "foundRequiredFacts":found,"officialCitation":cited,"result":page}));
    }
    library.close().await;
    let reopened = KnowledgeLibrary::open(&root, &modules).await?;
    for (module, id, offset, snippet, url, hash) in reads {
        let page = reopened.read(module, &id, offset).await?;
        assert!(page.body.starts_with(&snippet));
        assert_eq!(page.source.url, url);
        assert_eq!(page.source.content_sha256, hash);
    }
    reopened.close().await;
    std::fs::write(
        root.join("recovered-retrieval-report.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )?;
    for report in reports {
        assert_eq!(
            report["foundRequiredFacts"], true,
            "Missing setup facts: {report}"
        );
        assert_eq!(
            report["officialCitation"], true,
            "Missing official citation: {report}"
        );
    }
    Ok(())
}

/// LAN can formulate technical search terms in the document language in its
/// existing tool call. Verify the actual corpus, not authored summary fixtures.
#[tokio::test]
#[ignore = "requires LANGAME_KNOWLEDGE_LIVE_ROOT with synchronized public documents"]
async fn real_cached_corpus_technical_query_retrieval() -> Result<()> {
    let root = PathBuf::from(std::env::var("LANGAME_KNOWLEDGE_LIVE_ROOT").unwrap());
    assert!(root.is_absolute());
    let modules = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let library = KnowledgeLibrary::open(&root, &modules).await?;
    let mut results = Vec::new();
    for (module, query, expected_terms) in [
        (
            "palworld",
            "set server password maximum players",
            ["ServerPassword", "ServerPlayerMaxNum"],
        ),
        (
            "unturned",
            "Internet server UDP ports port forwarding",
            ["27015", "27016"],
        ),
    ] {
        let result = library.search(module, query, 0).await?;
        let evidence = result
            .entries
            .iter()
            .map(|entry| entry.snippet.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let found = expected_terms.iter().all(|term| evidence.contains(term));
        results.push(serde_json::json!({"module":module,"query":query,"foundRequiredFacts":found,"result":result}));
    }
    library.close().await;
    std::fs::write(
        root.join("technical-retrieval-report.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )?;
    for result in results {
        assert_eq!(
            result["foundRequiredFacts"], true,
            "Technical retrieval missed the requested setup facts: {result}"
        );
    }
    Ok(())
}

/// Runs the production downloader, parser, incremental store and learned index
/// against public sources. The path is deliberately explicit and isolated from
/// the user's LGSM database. Publisher failures remain visible in the receipt.
#[tokio::test]
#[ignore = "requires LANGAME_KNOWLEDGE_LIVE_ROOT; downloads public documents and the pinned model"]
async fn real_official_corpus_sync_and_retrieval() -> Result<()> {
    let root = PathBuf::from(
        std::env::var("LANGAME_KNOWLEDGE_LIVE_ROOT").expect("explicit isolated cache path"),
    );
    assert!(root.is_absolute());
    let modules = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let library = KnowledgeLibrary::open(&root, &modules).await?;
    // A comma-separated subset keeps affected-source verification in one
    // process/model load. Omission still exercises the complete catalog.
    let selected = std::env::var("LANGAME_KNOWLEDGE_LIVE_GAME").ok();
    let games: Vec<&str> = selected
        .as_deref()
        .map(|value| value.split(',').map(str::trim).collect())
        .unwrap_or_default();
    assert!(games.len() <= 32 && games.iter().all(|game| !game.is_empty()));
    let progress = Arc::new(|progress: crate::SyncProgress| {
        if progress.phase != "model_download"
            || progress.downloaded_bytes == progress.total_download_bytes
        {
            println!(
                "KNOWLEDGE_PROGRESS={}",
                serde_json::to_string(&progress).unwrap()
            );
        }
    });
    let scopes: Vec<Option<&str>> = if games.is_empty() {
        vec![None]
    } else {
        games.iter().map(|game| Some(*game)).collect()
    };
    let mut report = crate::SyncReport {
        started_at: crate::unix_seconds(),
        ..Default::default()
    };
    for scope in scopes {
        let result = library
            .sync(
                scope,
                true,
                Arc::new(AtomicBool::new(false)),
                progress.clone(),
            )
            .await?;
        report.sources_succeeded += result.sources_succeeded;
        report.sources_failed += result.sources_failed;
        report.changed_documents += result.changed_documents;
        report.cancelled |= result.cancelled;
        report.errors.extend(result.errors);
        if report.cancelled {
            break;
        }
    }
    report.finished_at = crate::unix_seconds();
    let status = library.status().await?;
    std::fs::write(
        root.join("sync-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )?;
    std::fs::write(
        root.join("source-status.json"),
        serde_json::to_vec_pretty(&status).unwrap(),
    )?;
    println!(
        "KNOWLEDGE_REPORT={}",
        serde_json::to_string(&report).unwrap()
    );
    let mut queries = Vec::new();
    for (module, query) in [
        (
            "palworld",
            "我只想让朋友进来玩，怎么给服务器设置密码和人数上限？",
        ),
        (
            "minecraft",
            "怎样限制陌生玩家加入我的服务器，只允许朋友进入？",
        ),
        ("vrising", "换电脑之前应该备份服务器的哪些文件？"),
        (
            "unturned",
            "朋友要通过互联网连接到我的服务器，需要开放哪些端口？",
        ),
        ("returntomoria", "如何设置专用服务器并让朋友加入？"),
    ] {
        if !games.is_empty() && !games.contains(&module) {
            continue;
        }
        match library.search(module, query, 0).await {
            Ok(page) => {
                for entry in &page.entries {
                    let read = library.read(module, &entry.id, entry.offset_bytes).await;
                    if entry.source.content_use == crate::ContentUse::Reference {
                        assert!(entry.snippet.chars().count() <= 400);
                        assert!(
                            matches!(read, Err(crate::KnowledgeError::Policy(ref reason)) if reason.contains(&entry.source.url))
                        );
                        continue;
                    }
                    let read = read?;
                    assert!(read.body.starts_with(&entry.snippet));
                    assert_eq!(read.source.content_sha256, entry.source.content_sha256);
                }
                queries.push(serde_json::json!({"module":module,"query":query,"result":page}));
            }
            Err(error) => queries
                .push(serde_json::json!({"module":module,"query":query,"error":error.to_string()})),
        }
    }
    std::fs::write(
        root.join("retrieval-report.json"),
        serde_json::to_vec_pretty(&queries).unwrap(),
    )?;
    println!(
        "KNOWLEDGE_COVERAGE={}",
        serde_json::json!({"gamesWithBodies":status.games.iter().filter(|game| game.sources.iter().any(|source| source.document_count>0)).count(),"documents":status.games.iter().flat_map(|game|&game.sources).map(|source|source.document_count).sum::<u64>(),"chunks":status.games.iter().flat_map(|game|&game.sources).map(|source|source.chunk_count).sum::<u64>()})
    );
    assert!(
        report.sources_succeeded > 0,
        "No publisher source was successfully synchronized"
    );
    library.close().await;
    Ok(())
}
