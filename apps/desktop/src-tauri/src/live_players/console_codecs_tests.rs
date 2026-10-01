use super::*;

fn parse_necesse(text: &str) -> Result<ParsedPlayerList, String> {
    parse(
        ModulePlayerListCodec::NecessePlayers,
        text,
        "request",
        1_000,
    )
}

#[test]
fn necesse_reads_counted_names_without_exposing_auth_or_connection() {
    let parsed = parse_necesse(include_str!(
        "../../test-data/live-players/necesse/players.txt"
    ))
    .expect("counted response");
    assert!(parsed.complete);
    assert!(!parsed.truncated);
    assert_eq!(parsed.current_players, Some(2));
    assert_eq!(parsed.max_players, Some(8));
    assert_eq!(parsed.entries[0].display_name, "Alice");
    assert_eq!(parsed.entries[1].display_name, "Bob \"Builder\"");
    assert_eq!(parsed.entries[0].ping_ms, Some(42));
    assert_eq!(parsed.entries[1].ping_ms, None);
    assert!(parsed.bindings.is_empty());
    for entry in parsed.entries {
        assert!(entry.identifiers.is_empty());
        assert!(entry.available_action_ids.is_empty());
        assert!(entry.attributes.is_empty());
    }
}

#[test]
fn necesse_empty_requires_a_complete_count_header() {
    assert!(parse_necesse("startup finished\n").is_err());
    assert!(parse_necesse("Players online: 0/8").is_err());
    let parsed = parse_necesse(include_str!(
        "../../test-data/live-players/necesse/empty.txt"
    ))
    .expect("zero count");
    assert!(parsed.complete);
    assert!(parsed.entries.is_empty());
    assert_eq!(parsed.current_players, Some(0));
    assert!(
        parse_necesse("\u{1b}[39m[2026-09-07 10:00:00] Players online: 0/8\n")
            .expect("native stdout ANSI prefix")
            .complete
    );
}

#[test]
fn necesse_rejects_missing_malformed_duplicate_and_excess_rows() {
    let row = "Slot 1: 10000000000000001 \"Alice\", latency: 42, level: 0x0d0,conn: LOCAL\n";
    for text in [
        format!("Players online: 2/8\n{row}"),
        format!("Players online: 2/8\n{row}{row}"),
        format!("Players online: 0/8\n{row}"),
        format!("Players online: 1/8\n{}", row.trim_end()),
        String::from("Players online: 1/8\nSlot 1: malformed\n"),
        String::from("Players online: 1/8\nunrelated log\n"),
        String::from("Players online: 9/8\n"),
        String::from("Players online: 1/5000\n"),
    ] {
        assert!(parse_necesse(&text).is_err(), "must reject: {text}");
    }
}

#[test]
fn console_capture_is_bounded_and_never_silently_discards_unknown_names() {
    assert!(parse_necesse(&"x".repeat(MAX_BYTES + 1)).is_err());
    assert!(parse_necesse("Players online: 0/8\0\n").is_err());
    let mut text = String::from("Players online: 257/257\n");
    for slot in 1..=257 {
        text.push_str(&format!(
            "Slot {slot}: {slot} \"Player {slot}\", latency: 0, level: 0x0d0,conn: LOCAL\n"
        ));
    }
    let parsed = parse_necesse(&text).expect("all rows captured");
    assert!(parsed.truncated);
    assert!(!parsed.complete);
    assert_eq!(parsed.entries.len(), MAX_VISIBLE_ROWS);
    assert_eq!(parsed.current_players, Some(257));
}

#[test]
fn romestead_reads_complete_counted_names_without_network_metadata() {
    let parsed = parse(
        ModulePlayerListCodec::RomesteadPlayers,
        include_str!("../../test-data/live-players/romestead/players.txt"),
        "romestead",
        1_000,
    )
    .expect("counted Romestead response");
    assert_eq!(parsed.current_players, Some(2));
    assert!(parsed.complete);
    assert!(parsed.bindings.is_empty());
    assert_eq!(parsed.entries[0].display_name, "Alice");
    assert_eq!(parsed.entries[1].display_name, "Bob (Builder)");
    assert!(parsed.entries.iter().all(|row| row.identifiers.is_empty()
        && row.available_action_ids.is_empty()
        && row.attributes.is_empty()));
    let empty = parse(
        ModulePlayerListCodec::RomesteadPlayers,
        include_str!("../../test-data/live-players/romestead/empty.txt"),
        "empty",
        1_000,
    )
    .expect("zero count");
    assert!(empty.complete);
    assert!(empty.entries.is_empty());
}

#[test]
fn romestead_rejects_truncation_duplicate_peer_and_unrelated_output() {
    let row = "Alice (1) - {X:0 Y:0} - Peer 3 - 192.0.2.1:8050\n";
    for text in [
        String::from("There are 0 players online:"),
        format!("There are 1 players online:\n{}", row.trim_end()),
        format!("There are 2 players online:\n{row}"),
        format!("There are 2 players online:\n{row}{row}"),
        format!("There are 0 players online:\n{row}"),
        String::from("There are 1 players online:\nAlice joined the server\n"),
    ] {
        assert!(parse(ModulePlayerListCodec::RomesteadPlayers, &text, "bad", 0).is_err());
    }
}

#[test]
fn terraria_uses_count_footer_and_keeps_names_with_parentheses() {
    let parsed = parse(
        ModulePlayerListCodec::TerrariaPlayers,
        include_str!("../../test-data/live-players/terraria/players.txt"),
        "terraria",
        1_000,
    )
    .expect("count footer");
    assert_eq!(parsed.current_players, Some(2));
    assert!(parsed.complete);
    assert_eq!(parsed.entries[1].display_name, "Bob (Builder)");
    assert!(
        parsed
            .entries
            .iter()
            .all(|row| row.identifiers.is_empty() && row.available_action_ids.is_empty())
    );
}

#[test]
fn terraria_accepts_every_shipped_language_count_template() {
    let templates = include_str!("../../test-data/live-players/terraria/counts.txt");
    for line in templates.lines() {
        let (expected, footer) = line.split_once('|').expect("count fixture");
        let expected = expected.parse::<usize>().expect("count");
        let rows = (0..expected)
            .map(|i| format!("Player {i} (192.0.2.1:{})\n", 7000 + i))
            .collect::<String>();
        let parsed = parse(
            ModulePlayerListCodec::TerrariaPlayers,
            &format!("{rows}{footer}\n"),
            "language",
            0,
        )
        .expect(footer);
        assert_eq!(parsed.current_players, Some(expected));
        assert!(parsed.complete);
    }
}

#[test]
fn terraria_requires_complete_rows_and_footer_before_publishing() {
    for text in [
        "No players connected.",
        "Alice (192.0.2.1:7777)\n",
        "1 player connected.\n",
        "Alice (192.0.2.1:7777)\n2 players connected.\n",
        "Alice (192.0.2.1:7777)\nNo players connected.\n",
        "Alice (192.0.2.1:7777)\nBob (192.0.2.2:7777)\n1 player connected.\n",
        "Alice joined\n1 player connected.\n",
        "Alice ()\n1 player connected.\n",
    ] {
        assert!(parse(ModulePlayerListCodec::TerrariaPlayers, text, "bad", 0).is_err());
    }
}
