use super::*;

fn html(text: &str) -> String {
    render(text, 322330).unwrap().unwrap()
}

#[test]
fn official_store_animation_attributes_retain_both_formats_and_layout() {
    // Shape and asset identity observed in StoreBrowse's 2026-10-01 DST response.
    let source = r#"[img src="{STEAM_APP_IMAGE}/extras/ab42e93c5610ff5c234e67ae9f3a8c4a.poster.avif" poster="{STEAM_APP_IMAGE}/extras/ab42e93c5610ff5c234e67ae9f3a8c4a.poster.avif" width="600" height="160" mp4="{STEAM_APP_IMAGE}/extras/ab42e93c5610ff5c234e67ae9f3a8c4a.mp4" webm="{STEAM_APP_IMAGE}/extras/ab42e93c5610ff5c234e67ae9f3a8c4a.webm" fromclient=1][/img]
中文说明。

Another paragraph."#;
    let rendered = html(source);
    let base = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/extras/ab42e93c5610ff5c234e67ae9f3a8c4a";
    assert_eq!(
        rendered,
        format!(
            "<video autoplay loop muted playsinline width=\"600\" height=\"160\" poster=\"{base}.poster.avif\"><source src=\"{base}.webm\" type=\"video/webm\" /><source src=\"{base}.mp4\" type=\"video/mp4\" /></video><br />中文说明。<br /><br />Another paragraph."
        )
    );
    assert!(!rendered.contains("fromclient"));
}

#[test]
fn paragraphs_images_and_both_steam_list_forms_keep_their_complete_text() {
    assert_eq!(
        html("[p][b][u]Heading[/u][/b][/p][list][*][p]一[/p][/*][*]Two[/*][/list]"),
        "<p><b><u>Heading</u></b></p><ul><li><p>一</p></li><li>Two</li></ul>"
    );
    assert_eq!(
        html("[h2]Features[/h2][olist][*]First[*][i]Second[/i][/olist]"),
        "<h2>Features</h2><ol><li>First</li><li><i>Second</i></li></ol>"
    );
    assert_eq!(
        html(
            r#"[p][img src="{STEAM_APP_IMAGE}/extras/header.avif" width="616" height="143" avif="{STEAM_APP_IMAGE}/extras/header.avif" fromclient=1][/img][/p]"#
        ),
        "<p><img width=\"616\" height=\"143\" src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/extras/header.avif\" /></p>"
    );
    assert_eq!(
        html("[img]//shared.akamai.steamstatic.com/banner.jpg[/img]"),
        "<img src=\"https://shared.akamai.steamstatic.com/banner.jpg\" />"
    );
}

#[test]
fn escaped_markup_unknown_tags_and_malformed_input_never_erase_the_body() {
    assert_eq!(
        html("<script>alert('x')</script>&\"[future=1]保留[b]text[/b][/future]"),
        "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;&quot;[future=1]保留<b>text</b>[/future]"
    );
    assert_eq!(
        html("A[unknown [b]B[/b]C[unclosed"),
        "A[unknown <b>B</b>C[unclosed"
    );
    assert_eq!(html("[b]unclosed正文"), "<b>unclosed正文</b>");
    assert_eq!(html("[/p]retained"), "[/p]retained");
    assert_eq!(
        html("one\r\ntwo\rthree\nfour"),
        "one<br />two<br />three<br />four"
    );
}

#[test]
fn unicode_and_malformed_quoted_tags_keep_text_without_invalid_byte_slices() {
    let rendered =
        html(r#"前缀🦀[img src="https://example.com/中文.avif" alt="中文🦀"][/img]后缀"#);
    assert!(rendered.starts_with("前缀🦀<img"));
    assert!(rendered.contains("alt=\"中文🦀\""));
    assert!(rendered.ends_with("后缀"));
    assert_eq!(
        html(r#"前[url="没闭合🦀]正文[b]末尾[/b]"#),
        "前[url=&quot;没闭合🦀]正文[b]末尾[/b]"
    );
    assert_eq!(html("[b]甲[i]乙[/b]丙[/i]丁"), "<b>甲<i>乙[/b]丙</i>丁</b>");
    assert_eq!(
        html("[url=https://example.com/O'Brien]说明[/url]"),
        "<a href=\"https://example.com/O&#39;Brien\" target=\"_blank\" rel=\"noreferrer\">说明</a>"
    );
}

#[test]
fn urls_and_attributes_cannot_introduce_active_markup() {
    assert_eq!(
        html(r#"[url="https://example.com/?a=1&b=2"]<safe>[/url]"#),
        "<a href=\"https://example.com/?a=1&amp;b=2\" target=\"_blank\" rel=\"noreferrer\">&lt;safe&gt;</a>"
    );
    for value in [
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///c:/secret",
        "https://user:pass@example.com/",
        "https://example.com/\nattack",
    ] {
        let link = html(&format!("[url={value}]visible[/url]"));
        assert!(!link.contains("<a "), "{link}");
        assert!(link.contains("visible"));
        let image = html(&format!("[img]{value}[/img]"));
        assert!(!image.contains("<img"), "{image}");
    }
    let rendered = html(
        r#"[img src='https://example.com/a"onerror="alert(1)' alt='" onload="x' width="0" height="100000" onerror="bad"][/img]"#,
    );
    assert!(rendered.contains("%22"));
    assert!(rendered.contains("alt=\"&quot; onload=&quot;x\""));
    assert!(!rendered.contains(" onerror="));
    assert!(!rendered.contains(" width="));
    assert!(!rendered.contains(" height="));
    assert!(
        html(r#"[img src="https://example.com/a" src="javascript:bad"]text[/img]"#)
            .contains("text")
    );
}

#[test]
fn animation_can_use_one_valid_format_and_retains_unexpected_body_copy() {
    let rendered = html(
        r#"[img src="https://example.com/p.avif" mp4="https://example.com/a.mp4" webm="javascript:bad"]caption & text[/img]"#,
    );
    assert!(rendered.contains("<video autoplay loop muted playsinline"));
    assert!(rendered.contains("type=\"video/mp4\""));
    assert!(!rendered.contains("video/webm"));
    assert!(rendered.ends_with("</video>caption &amp; text"));
}

#[test]
fn input_nesting_tokens_and_expansion_have_explicit_limits() {
    assert_eq!(render(" \r\n ", 322330).unwrap(), None);
    assert!(render("text", 0).is_err());
    assert!(
        render(&"a".repeat(MAX_INPUT_BYTES + 1), 322330)
            .unwrap_err()
            .contains("byte limit")
    );
    assert!(
        render(
            &format!(
                "{}text{}",
                "[b]".repeat(MAX_DEPTH),
                "[/b]".repeat(MAX_DEPTH)
            ),
            322330
        )
        .is_ok()
    );
    assert!(
        render(&"[b]".repeat(MAX_DEPTH + 1), 322330)
            .unwrap_err()
            .contains("nesting")
    );
    assert!(
        render(&"[br]".repeat(MAX_TOKENS + 1), 322330)
            .unwrap_err()
            .contains("token")
    );
    let mut output = Html("x".repeat(MAX_OUTPUT_BYTES));
    assert!(
        output
            .text("&", false)
            .unwrap_err()
            .contains("rendered byte")
    );
}
