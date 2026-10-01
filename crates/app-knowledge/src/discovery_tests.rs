use crate::{extract, sources};
use reqwest::Url;

fn directory_source() -> sources::Source {
    toml::from_str(
        r#"
id = "publisher"
title = "Dedicated server manual"
authority = "Publisher"
kind = "official"
seeds = ["https://docs.example.com/server-guides/"]
allowed_prefixes = ["https://docs.example.com/server-guides/", "https://docs.example.com/articles/"]
discover_links = true
max_pages = 128
content_selector = ".directory, .article"
discovery_selector = ".server-doc-links a[href], .directory-pages a[href]"
authority_evidence = "https://docs.example.com/about"
license_note = "Public publisher documentation"
reviewed_on = "2026-09-28"
"#,
    )
    .unwrap()
}

#[test]
fn reviewed_directory_discovers_new_articles_and_pages_without_importing_sidebar_recommendations() {
    let source = directory_source();
    let page = extract::extract(
        &source,
        &Url::parse(&source.seeds[0]).unwrap(),
        "text/html",
        br#"<main><div class="directory"><h1>Server guides</h1>
        <ul class="server-doc-links"><li><a href="/articles/added-after-release">New server guide</a></li></ul>
        </div><nav class="directory-pages"><a href="/server-guides/page/2">Next</a></nav>
        <aside><a href="/articles/client-only">Client troubleshooting</a></aside></main>"#,
    ).unwrap();
    assert_eq!(
        page.links,
        [
            "https://docs.example.com/articles/added-after-release",
            "https://docs.example.com/server-guides/page/2",
        ]
    );
    assert!(
        page.body.is_empty(),
        "Directory navigation is not a server manual"
    );
}

#[test]
fn discovered_articles_are_terminal_even_when_the_sidebar_links_to_other_faqs() {
    let page = extract::extract(
        &directory_source(),
        &Url::parse("https://docs.example.com/articles/new-server-guide").unwrap(),
        "text/html",
        br#"<article class="article"><h1>Preserve the world</h1><p>Stop the server before copying its world and configuration files.</p></article>
        <aside><a href="/articles/client-only">Client troubleshooting</a></aside>"#,
    ).unwrap();
    assert!(page.links.is_empty());
    assert!(page.body.contains("Stop the server before copying"));
}

#[test]
fn broken_directory_selector_is_a_failure_instead_of_replacing_documents_with_an_empty_listing() {
    let source = directory_source();
    let pagination = extract::extract(
        &source,
        &Url::parse("https://docs.example.com/server-guides/page/2").unwrap(),
        "text/html",
        b"<div class='directory'>Server guides page two: the link layout changed</div>",
    );
    assert!(
        matches!(pagination, Err(ref error) if error.to_string().contains("no longer yields in-scope links")),
        "Failed directory pagination must retain the previous complete source"
    );
    let redirected = extract::extract(
        &source,
        &Url::parse("https://docs.example.com/server-guides").unwrap(),
        "text/html",
        b"<div class='directory'>The layout changed</div>",
    );
    assert!(
        redirected.is_err(),
        "A canonical trailing-slash redirect must not bypass directory validation"
    );
    for html in [
        "<div class='directory'>The layout changed</div>",
        "<div class='directory'><div class='server-doc-links'><a href='https://other.example.com/manual'>External</a></div></div>",
        "<div class='directory'><div class='server-doc-links'><a href='/server-guides/'>Self</a></div></div>",
    ] {
        let error = extract::extract(
            &source,
            &Url::parse(&source.seeds[0]).unwrap(),
            "text/html",
            html.as_bytes(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no longer yields in-scope links")
        );
    }
}
