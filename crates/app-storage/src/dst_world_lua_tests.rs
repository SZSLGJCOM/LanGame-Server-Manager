use super::*;

#[test]
fn static_data_preserves_unknown_values_comments_and_string_forms() {
    let source = "-- header\nreturn { overrides = { ['day'] = 'onlyday', -- chosen\n custom = {false, 2.5e-1, [4] = [=[\nhello }]=], note = \"\\065\\x42\\z  C\"} }, preset = 'ENDLESS' }";
    let root = parse(source, false).expect("static world data");
    let overrides = root.get("overrides").unwrap().table().unwrap();
    assert_eq!(
        overrides.get("day").unwrap().scalar(),
        Some(&Value::String(String::from("onlyday")))
    );
    let patched = set_fields(
        source,
        overrides,
        &BTreeMap::from([(String::from("day"), quote("default"))]),
    );
    assert_eq!(patched, source.replace("'onlyday'", "\"default\""));
    assert!(parse(&patched, false).is_some());
}

#[test]
fn insertion_immediately_before_closing_braces_keeps_a_separator() {
    let source = "return {override_enabled=true,overrides={day='default'}}";
    let root = parse(source, false).unwrap();
    let nested = set_fields(
        source,
        root.get("overrides").unwrap().table().unwrap(),
        &BTreeMap::from([(String::from("rain"), quote("always"))]),
    );
    let root = parse(&nested, false).expect("new nested field is separated");
    let patched = set_fields(
        &nested,
        &root,
        &BTreeMap::from([(String::from("settings_preset"), quote("ENDLESS"))]),
    );
    assert!(
        parse(&patched, false).is_some(),
        "new top-level field is separated: {patched}"
    );
    let fragment = "day='default' -- trailing comment";
    let root = parse(fragment, true).unwrap();
    let patched = set_fields(
        fragment,
        &root,
        &BTreeMap::from([(String::from("rain"), quote("always"))]),
    );
    assert!(patched.contains("'default', -- trailing comment"));
    assert!(parse(&patched, true).is_some());
}

#[test]
fn parser_rejects_executable_ambiguous_and_unbounded_inputs() {
    for source in [
        "return build_world()",
        "local x = {}; return x",
        "return {x=1+2}",
        "return {x='a', ['x']='b'}",
        "return {1, [1]=2}",
        "return {[1]=2,[1.0]=3}",
        "return {x=1e9999}",
        "return {x=0x10}",
        "return {x=function() end}",
        "return {x='unterminated}",
        "return {x=[[unterminated}",
        "return {x='\\u{41}'}",
        "return {true=1}",
        "return {x=true}\u{00a0}",
    ] {
        assert!(
            parse(source, false).is_none(),
            "unexpected static parse: {source}"
        );
    }
    assert!(
        parse(
            &format!("return {{ x='{}' }}", "x".repeat(MAX_BYTES)),
            false
        )
        .is_none()
    );
    assert!(
        parse(
            &format!(
                "return {}{}",
                "{".repeat(MAX_DEPTH + 1),
                "}".repeat(MAX_DEPTH + 1)
            ),
            false
        )
        .is_none()
    );
    assert!(
        parse(
            &format!("return {{{}}}", "1,".repeat(MAX_ENTRIES + 1)),
            false
        )
        .is_none()
    );
    assert!(parse(&format!("return {{{}}}", "1,".repeat(MAX_ENTRIES)), false).is_some());
}

#[test]
fn quoting_round_trips_lua_text_without_json_unicode_escape_syntax() {
    let text = "quote\" slash\\ control\0\u{001f} 中文\n";
    let source = format!("return {{ text={} }}", quote(text));
    let parsed = parse(&source, false).unwrap();
    assert_eq!(
        parsed.get("text").unwrap().scalar(),
        Some(&Value::String(text.to_string()))
    );
}

#[test]
fn lua_whitespace_negative_literals_and_utf8_byte_escapes_are_static() {
    let source = "return\u{000b}{text='\\195\\169', number=- -- note\n 2, known='\\x64efault', raw=[=[\n\rline\n\rnext]=]}";
    let table = parse(source, false).unwrap();
    assert_eq!(
        table.get("text").unwrap().scalar(),
        Some(&Value::String(String::from("é")))
    );
    assert_eq!(
        table.get("known").unwrap().scalar(),
        Some(&Value::String(String::from("default")))
    );
    assert_eq!(
        table.get("raw").unwrap().scalar(),
        Some(&Value::String(String::from("line\nnext")))
    );
    assert_eq!(
        table.get("number").unwrap().scalar().unwrap().as_f64(),
        Some(-2.0)
    );
}
