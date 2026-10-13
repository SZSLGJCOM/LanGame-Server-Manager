#![cfg(all(test, feature = "custom-protocol"))]

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use dom_query::Document;
use tauri::utils::assets::AssetKey;
use tauri::utils::config::{Csp, CspDirectiveSources, FrontendDist};

fn frontend_code_files(directory: &Path) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for entry in std::fs::read_dir(directory).expect("read frontend build") {
        let entry = entry.expect("read frontend entry");
        let path = entry.path();
        if entry.file_type().expect("frontend entry type").is_dir() {
            files.extend(frontend_code_files(&path));
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("js" | "css")
        ) {
            files.insert(path);
        }
    }
    files
}

fn entry_references(document: &Document) -> BTreeSet<String> {
    document
        .select("script[src], link[rel=stylesheet], link[rel=modulepreload]")
        .iter()
        .map(|node| {
            node.attr("src")
                .or_else(|| node.attr("href"))
                .expect("frontend entry reference")
                .to_string()
        })
        .collect()
}

#[test]
fn embedded_frontend_preserves_built_chunks_and_content_security_policy() {
    let context: tauri::Context<tauri::test::MockRuntime> = tauri::tauri_build_context!();
    let Some(FrontendDist::Directory(dist)) = &context.config().build.frontend_dist else {
        panic!("desktop custom protocol requires a frontend directory");
    };
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join(dist);
    let source_html = std::fs::read_to_string(dist.join("index.html")).expect("built index");
    let source = Document::from(source_html.as_str());
    let references = entry_references(&source);
    assert!(
        !references.is_empty(),
        "built entry must load frontend code"
    );

    // Exact asset lookup cannot silently return index.html for a missing chunk.
    let files = frontend_code_files(&dist);
    assert!(!files.is_empty(), "frontend build must contain code");
    for file in files {
        let key = AssetKey::from(file.strip_prefix(&dist).unwrap());
        let actual = context.assets().get(&key).expect("embedded frontend chunk");
        assert_eq!(
            actual.as_ref(),
            std::fs::read(&file).unwrap(),
            "{}",
            file.display()
        );
    }
    for reference in &references {
        assert!(
            context.assets().get(&AssetKey::from(reference)).is_some(),
            "{reference}"
        );
    }
    assert!(
        context
            .assets()
            .get(&AssetKey::from("index.html"))
            .is_some()
    );
    let source_config: tauri::Config =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("desktop configuration");
    assert_eq!(
        context.config().app.security.csp,
        source_config.app.security.csp
    );
    let expected_csp: HashMap<String, CspDirectiveSources> =
        source_config.app.security.csp.expect("desktop CSP").into();
    let app = tauri::test::mock_builder()
        .build(context)
        .expect("asset resolver");
    let asset = app
        .asset_resolver()
        .get_for_scheme("index.html".into(), false)
        .expect("resolved index");
    assert_eq!(asset.mime_type(), "text/html");
    let resolved_html = std::str::from_utf8(asset.bytes()).expect("UTF-8 index");
    let resolved = Document::from(resolved_html);
    assert_eq!(resolved.select("#root").length(), 1);
    assert_eq!(
        resolved.select("title").text(),
        source.select("title").text()
    );
    assert_eq!(entry_references(&resolved), references);
    let actual_csp: HashMap<String, CspDirectiveSources> =
        Csp::Policy(asset.csp_header().expect("resolved CSP").to_owned()).into();
    for (directive, sources) in expected_csp {
        let expected: Vec<String> = sources.into();
        let actual: Vec<String> = actual_csp
            .get(&directive)
            .expect("CSP directive")
            .clone()
            .into();
        assert!(
            expected.iter().all(|source| actual.contains(source)),
            "{directive}"
        );
    }
    assert!(!resolved_html.contains(tauri::utils::assets::SCRIPT_NONCE_TOKEN));
    assert!(!resolved_html.contains(tauri::utils::assets::STYLE_NONCE_TOKEN));
}
