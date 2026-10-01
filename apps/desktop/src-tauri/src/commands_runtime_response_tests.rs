use super::runtime_action_response_text;

#[test]
fn internal_responses_preserve_names_and_bypass_only_the_preview_limit() {
    let text = format!("Players connected (1):\n-{}  \n", "x".repeat(20_000));
    assert_eq!(
        runtime_action_response_text(text.clone(), true).unwrap(),
        Some(text.clone())
    );
    let preview = runtime_action_response_text(text, false).unwrap().unwrap();
    assert!(preview.ends_with("[LanGame: response truncated at 16384 characters]"));
    assert!(runtime_action_response_text("x".repeat(512 * 1024 + 1), true).is_err());
    assert!(runtime_action_response_text("汉".repeat(180_000), true).is_err());
    assert_eq!(
        runtime_action_response_text(" \r\n".into(), true).unwrap(),
        None
    );
}
