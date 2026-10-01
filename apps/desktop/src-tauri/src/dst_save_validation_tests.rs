use super::*;

#[test]
fn dst_shardindex_parser_reads_official_klei_serialization() {
    let input = br#"KLEI     1 return {
        server = { session_id = "DECOY" },
        -- session_id = "COMMENT"
        note = "session_id = 'STRING'",
        session_id = "3BE1218FE7B5A329",
    }"#;

    assert_eq!(
        parse_top_level_session_id(input),
        Ok(String::from("3BE1218FE7B5A329"))
    );
}

#[test]
fn dst_shardindex_parser_rejects_nested_or_unsafe_session_ids() {
    assert_eq!(
        parse_top_level_session_id(br#"return { nested = { session_id = "DECOY" } }"#),
        Err("shardindex has no top-level session_id")
    );
    assert_eq!(
        parse_top_level_session_id(br#"return { session_id = "../outside" }"#),
        Err("shardindex session_id is not a safe session directory name")
    );
}

#[test]
fn dst_shardindex_parser_ignores_long_strings_and_comments() {
    let input = br#"KLEI 1 return {
        description = [=[ } session_id = "DECOY" ]=],
        --[==[ session_id = "COMMENT" } ]==]
        session_id = "3BE1218FE7B5A329",
    }"#;

    assert_eq!(
        parse_top_level_session_id(input),
        Ok(String::from("3BE1218FE7B5A329"))
    );
}

#[test]
fn dst_shardindex_parser_rejects_excessive_nesting() {
    let mut input = String::from("return {");
    input.push_str(&"{".repeat(MAX_SHARDINDEX_DEPTH));
    input.push_str(&"}".repeat(MAX_SHARDINDEX_DEPTH + 1));

    assert_eq!(
        parse_top_level_session_id(input.as_bytes()),
        Err("shardindex exceeds the nesting depth limit")
    );
}

#[test]
#[ignore = "requires LANGAME_DST_SAVE_ROOT pointing to an operator-selected native shard save"]
fn dst_save_validator_accepts_an_external_native_save() {
    let save_root = std::env::var_os("LANGAME_DST_SAVE_ROOT")
        .map(std::path::PathBuf::from)
        .expect("LANGAME_DST_SAVE_ROOT must point to a native shard save directory");
    let mut budget = 8192;

    assert_eq!(
        inspect_dst_shard_save(&save_root, &mut budget).unwrap(),
        DstShardSaveInspection::Valid
    );
}
