use super::*;

#[test]
fn withdrawn_anonymous_articles_hide_cached_evidence_but_network_failures_do_not() {
    let modules = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let catalog = crate::sources::load(&modules, "enshrouded").unwrap();
    let source = &catalog.sources[0];
    let url = Url::parse(&source.seeds[0]).unwrap();
    for code in [401, 403, 404, 410, 451] {
        assert!(matches!(
            check_article_access(source, &url, StatusCode::from_u16(code).unwrap()),
            Err(KnowledgeError::Policy(_))
        ));
    }
    for code in [200, 304, 429, 500, 503] {
        assert!(check_article_access(source, &url, StatusCode::from_u16(code).unwrap()).is_ok());
    }
}

#[test]
fn reference_is_indexable_but_immediate_and_rag_prohibitions_are_not() {
    assert_eq!(
        robots_policy(
            "User-agent: *\nContent-Signal: search=yes, ai-train=no, use=reference\nAllow: /",
            "/docs"
        )
        .unwrap(),
        ContentUse::Reference
    );
    for signal in ["use=immediate", "ai-input=no", "search=no"] {
        let policy = format!("User-agent: *\nContent-Signal: {signal}\nAllow: /");
        assert!(
            matches!(
                robots_policy(&policy, "/docs"),
                Err(KnowledgeError::Policy(_))
            ),
            "{signal}"
        );
    }
    assert_eq!(
        robots_policy(
            "User-agent: *\nContent-Signal: ai-train=no, use=full\nAllow: /",
            "/docs"
        )
        .unwrap(),
        ContentUse::Full
    );
}

#[test]
fn content_signals_follow_the_applicable_user_agent_groups() {
    let other_bot = "User-agent: GPTBot\nContent-Signal: ai-input=no\nDisallow: /\nUser-agent: *\nContent-Signal: use=reference\nAllow: /";
    assert_eq!(
        robots_policy(other_bot, "/docs").unwrap(),
        ContentUse::Reference
    );
    let specific = "User-agent: *\nContent-Signal: ai-input=no\nDisallow: /\nUser-agent: LanGameDocs\nContent-Signal: use=reference\nAllow: /";
    assert_eq!(
        robots_policy(specific, "/docs").unwrap(),
        ContentUse::Reference
    );
    let groups = "User-agent: LanGameDocs\nContent-Signal: use=full\nAllow: /\nUser-agent: LanGameDocs\nContent-Signal: use=reference\nDisallow: /private";
    assert_eq!(
        robots_policy(groups, "/docs").unwrap(),
        ContentUse::Reference
    );
    assert!(robots_policy(groups, "/private/config").is_err());
    let signal_only =
        "User-agent: OtherBot\nContent-Signal: use=immediate\nUser-agent: LanGameDocs\nAllow: /";
    assert_eq!(
        robots_policy(signal_only, "/docs").unwrap(),
        ContentUse::Full
    );
}

#[test]
fn signals_are_exact_fields_and_repeated_declarations_cannot_relax_a_limit() {
    assert_eq!(
        ContentUse::Full
            .apply("x-ai-input=no, use=reference, use=full")
            .unwrap(),
        ContentUse::Reference
    );
    assert!(
        ContentUse::Full
            .apply("use=full, ai-input=no, ai-input=yes")
            .is_err()
    );
    assert_eq!(
        ContentUse::Full
            .apply("search = yes, AI-INPUT = yes, USE = reference")
            .unwrap(),
        ContentUse::Reference
    );
}

#[tokio::test]
async fn response_headers_apply_reference_or_prohibition_to_documents_including_304() {
    let cancel = AtomicBool::new(false);
    for status in [200, 304] {
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(status)
                .header("content-signal", "use=reference")
                .header("content-signal", "use=full, ai-train=no")
                .body("manual".to_owned())
                .unwrap(),
        );
        let page = read_response(
            "https://example.com/docs".parse().unwrap(),
            response,
            1024,
            true,
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(page.content_use, ContentUse::Reference);
        for signal in ["ai-input=no", "search=no", "use=immediate"] {
            let response = reqwest::Response::from(
                http::Response::builder()
                    .status(status)
                    .header("content-signal", "use=full")
                    .header("content-signal", signal)
                    .body("manual".to_owned())
                    .unwrap(),
            );
            assert!(matches!(
                read_response(
                    "https://example.com/docs".parse().unwrap(),
                    response,
                    1024,
                    true,
                    &cancel
                )
                .await,
                Err(KnowledgeError::Policy(_))
            ));
        }
    }
    let response = reqwest::Response::from(
        http::Response::builder()
            .header("content-signal", "use=immediate")
            .body("User-agent: *\nAllow: /".to_owned())
            .unwrap(),
    );
    assert!(
        read_response(
            "https://example.com/robots.txt".parse().unwrap(),
            response,
            1024,
            false,
            &cancel
        )
        .await
        .is_ok()
    );
}

#[test]
fn unavailable_robots_is_distinct_from_rate_limits_and_server_failure() {
    for status in [400, 401, 403, 404, 410] {
        assert!(robots_unavailable(StatusCode::from_u16(status).unwrap()));
    }
    for status in [200, 301, 429, 451, 500, 503] {
        assert!(!robots_unavailable(StatusCode::from_u16(status).unwrap()));
    }
}

#[tokio::test]
async fn robots_noindex_does_not_revoke_other_pages_but_document_restrictions_do() {
    let response = || {
        reqwest::Response::from(
            http::Response::builder()
                .status(200)
                .header("x-robots-tag", "index")
                .header("x-robots-tag", "noindex")
                .body("User-agent: *\nAllow: /".to_owned())
                .unwrap(),
        )
    };
    let cancel = AtomicBool::new(false);
    let robots = read_response(
        "https://example.com/robots.txt".parse().unwrap(),
        response(),
        1024,
        false,
        &cancel,
    )
    .await
    .unwrap();
    assert!(robots_allowed(
        std::str::from_utf8(&robots.body).unwrap(),
        "/docs/setup"
    ));
    assert!(matches!(
        read_response(
            "https://example.com/docs/setup".parse().unwrap(),
            response(),
            1024,
            true,
            &cancel
        )
        .await,
        Err(KnowledgeError::Policy(_))
    ));
}
