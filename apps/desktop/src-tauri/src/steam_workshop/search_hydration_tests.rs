use serde_json::{Value, json};

use super::tests::{browse_query, page_html, parse};

fn old_page_html(query: Value) -> String {
    let context = json!({"queryData": json!({"queries": [query]}).to_string()}).to_string();
    format!(
        "<script>window.SSR.renderContext = JSON.parse({});</script>",
        serde_json::to_string(&context).expect("synthetic JSON")
    )
}

fn assert_unrecognized(html: &str) {
    let error = parse(html, 1).expect_err("invalid hydration must not become catalog data");
    let error: Value = serde_json::from_str(&error).expect("structured browse error");
    assert_eq!(error["code"], "steam_workshop_browse_unrecognized_response");
}

#[test]
fn json_script_accepts_html_attribute_variants_and_preserves_raw_json_text() {
    let mut query = browse_query(1);
    query["state"]["data"]["results"][0]["title"] = json!("显示 &amp; \"quoted\"");
    let query_data = json!({"queries": [query]})
        .to_string()
        .replace("显示", r"\u663e\u793a");
    let payload = json!({"renderContext": {"queryData": query_data}})
        .to_string()
        .replace("renderContext", r"\u0072enderContext");
    for attributes in [
        r#"type="application/json" id="valve-ssr-data""#,
        "id='valve-ssr-data'\n type = 'application/json'",
        "data-fixture='hydration'\n id = \"valve-ssr-data\"\t type = 'application/json'",
    ] {
        let html = format!("<script {attributes}>\n\t{payload}\n</script>");
        let (items, total, more) = parse(&html, 1).expect("JSON script catalog");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("显示 &amp; \"quoted\""));
        assert_eq!(total, 31);
        assert!(more);
    }
}

#[test]
fn json_script_takes_precedence_over_old_assignment_in_either_document_order() {
    let current = page_html(vec![browse_query(1)]);
    let mut decoy = browse_query(1);
    decoy["state"]["data"]["results"][0]["title"] = json!("obsolete assignment");
    let old = old_page_html(decoy);
    for html in [format!("{old}{current}"), format!("{current}{old}")] {
        let (items, _, _) = parse(&html, 1).expect("current JSON script catalog");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("伤害显示 \"Show Damage\""));
    }
}

#[test]
fn damaged_json_script_does_not_fall_back_to_a_valid_old_assignment() {
    let old = old_page_html(browse_query(1));
    for payload in [
        "",
        "{",
        "{}",
        r#"{"renderContext":null}"#,
        r#"{"renderContext":{}}"#,
        r#"{"renderContext":{"queryData":{}}}"#,
        r#"{"renderContext":{"queryData":"{"}}"#,
    ] {
        let current =
            format!("<script type='application/json' id='valve-ssr-data'>{payload}</script>");
        for html in [format!("{old}{current}"), format!("{current}{old}")] {
            assert_unrecognized(&html);
        }
    }
}

#[test]
fn duplicate_json_script_ids_and_wrong_script_types_are_rejected() {
    let current = page_html(vec![browse_query(1)]);
    let old = old_page_html(browse_query(1));
    let payload = json!({"renderContext": {
        "queryData": json!({"queries": [browse_query(1)]}).to_string()
    }});
    assert_unrecognized(&format!("{current}{current}{old}"));
    for attributes in [
        "id='valve-ssr-data'",
        "id='valve-ssr-data' type='text/javascript'",
        "id='valve-ssr-data' type='application/ld+json'",
    ] {
        let wrong_type = format!("<script {attributes}>{payload}</script>");
        assert_unrecognized(&format!("{wrong_type}{old}"));
        assert_unrecognized(&format!("{current}{wrong_type}{old}"));
    }
}

#[test]
fn old_assignment_remains_supported_when_json_script_is_absent() {
    let (items, total, more) =
        parse(&old_page_html(browse_query(1)), 1).expect("old assignment catalog");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "2964299587");
    assert_eq!(items[0].title.as_deref(), Some("伤害显示 \"Show Damage\""));
    assert_eq!(total, 31);
    assert!(more);
}

#[test]
fn comments_and_non_script_elements_cannot_supply_or_shadow_hydration() {
    let current = page_html(vec![browse_query(1)]);
    let noise =
        format!("<!--{current}--><div id='valve-ssr-data' type='application/json'>{{}}</div>");
    assert_unrecognized(&noise);
    for catalog in [current, old_page_html(browse_query(1))] {
        let (items, total, _) = parse(&format!("{noise}{catalog}"), 1)
            .expect("only actual script elements supply hydration");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "2964299587");
        assert_eq!(total, 31);
    }
}
