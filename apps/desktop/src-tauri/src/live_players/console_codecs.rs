use std::collections::{HashMap, HashSet};

use app_core::{ModulePlayerListCodec, RuntimeLivePlayerEntry};

use super::response_codecs::ParsedPlayerList;

#[cfg(test)]
#[path = "console_codecs_tests.rs"]
mod tests;

const MAX_BYTES: usize = 512 * 1024;
const MAX_ROWS: usize = 4096;
const MAX_VISIBLE_ROWS: usize = 256;
const MAX_LINE_BYTES: usize = 8192;

/// The caller supplies only stdout appended after dispatch, from one running generation.
/// A quiet log is never evidence of completeness: counted responses require every row.
pub(crate) fn parse(
    codec: ModulePlayerListCodec,
    text: &str,
    request_id: &str,
    _now: u64,
) -> Result<ParsedPlayerList, String> {
    if text.len() > MAX_BYTES || text.contains('\0') {
        return Err(String::from(
            "Console player output exceeds the capture limit or contains NUL.",
        ));
    }
    // Never interpret an unterminated log fragment as an authoritative empty/list response.
    let (complete_lines, _) = text
        .rsplit_once('\n')
        .ok_or("Console player output has no complete line.")?;
    let lines = complete_lines
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect::<Vec<_>>();
    if lines.iter().any(|line| line.len() > MAX_LINE_BYTES) {
        return Err(String::from(
            "Console player output exceeds the line limit.",
        ));
    }
    match codec {
        ModulePlayerListCodec::NecessePlayers => necesse(&lines, request_id),
        ModulePlayerListCodec::RomesteadPlayers => romestead(&lines, request_id),
        ModulePlayerListCodec::TerrariaPlayers => terraria(&lines, request_id),
        ModulePlayerListCodec::BarotraumaPlayers => super::barotrauma::parse(&lines, request_id),
        _ => Err(String::from("This codec has no console player parser.")),
    }
}

fn necesse(lines: &[&str], request_id: &str) -> Result<ParsedPlayerList, String> {
    let (start, header) = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            necesse_line(line)
                .strip_prefix("Players online: ")
                .map(|header| (index, header))
        })
        .ok_or("Necesse has not returned its online-player count header.")?;
    let (count, maximum) = header
        .split_once('/')
        .ok_or("Necesse player count is malformed.")?;
    let count = count_value(count)?;
    let maximum = count_value(maximum)?;
    if maximum == 0 || count > maximum {
        return Err(String::from(
            "Necesse player count exceeds the server capacity.",
        ));
    }
    let mut entries = Vec::new();
    let mut slots = HashSet::new();
    let mut previous_slot = 0;
    for line in lines.iter().skip(start + 1).take(count) {
        let row = necesse_line(line)
            .strip_prefix("Slot ")
            .ok_or("Necesse player rows are incomplete or interleaved.")?;
        let (slot, data) = row
            .split_once(": ")
            .ok_or("Necesse player slot is malformed.")?;
        let slot = count_value(slot)?;
        if slot == 0 || slot > maximum || slot <= previous_slot || !slots.insert(slot) {
            return Err(String::from(
                "Necesse player slot is duplicated or out of order.",
            ));
        }
        previous_slot = slot;
        let (auth, data) = data
            .split_once(" \"")
            .ok_or("Necesse player identity field is missing.")?;
        auth.parse::<i64>()
            .map_err(|_| "Necesse authentication field is malformed.")?;
        let (name, details) = data
            .rsplit_once("\", latency: ")
            .ok_or("Necesse player name or latency is missing.")?;
        let (latency, details) = details
            .split_once(", level: ")
            .ok_or("Necesse player level field is missing.")?;
        let (level, connection) = details
            .rsplit_once(",conn: ")
            .ok_or("Necesse player connection field is missing.")?;
        if level.is_empty() || connection.is_empty() {
            return Err(String::from("Necesse player details are incomplete."));
        }
        let latency = latency
            .parse::<i32>()
            .map_err(|_| "Necesse latency is malformed.")?;
        let entry = display_player(name, request_id, slots.len(), u32::try_from(latency).ok())?;
        if entries.len() < MAX_VISIBLE_ROWS {
            entries.push(entry);
        }
    }
    if slots.len() != count {
        return Err(String::from(
            "Necesse has not returned every counted player.",
        ));
    }
    if lines
        .get(start + count + 1)
        .is_some_and(|line| necesse_line(line).starts_with("Slot "))
    {
        return Err(String::from(
            "Necesse returned more players than its count header.",
        ));
    }
    Ok(finish(entries, count, Some(maximum)))
}

fn romestead(lines: &[&str], request_id: &str) -> Result<ParsedPlayerList, String> {
    let (start, count) = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            line.strip_prefix("There are ")
                .and_then(|header| header.strip_suffix(" players online:"))
                .map(|count| (index, count))
        })
        .ok_or("Romestead has not returned its online-player count header.")?;
    let count = count_value(count)?;
    let mut entries = Vec::new();
    let mut peers = HashSet::new();
    for line in lines.iter().skip(start + 1).take(count) {
        let (player, peer) = line
            .rsplit_once(" - Peer ")
            .ok_or("Romestead player connection field is incomplete.")?;
        let (peer_id, connection) = peer
            .split_once(" - ")
            .ok_or("Romestead player connection field is malformed.")?;
        let peer_id = decimal_id(peer_id)?;
        if connection.is_empty() || !peers.insert(peer_id) {
            return Err(String::from(
                "Romestead player connection is missing or duplicated.",
            ));
        }
        let (identity, position) = player
            .rsplit_once(") - ")
            .ok_or("Romestead player position is missing.")?;
        let (name, character_id) = identity
            .rsplit_once(" (")
            .ok_or("Romestead player name or ID is missing.")?;
        decimal_id(character_id)?;
        if position.is_empty() {
            return Err(String::from("Romestead player position is incomplete."));
        }
        let entry = display_player(name, request_id, peers.len(), None)?;
        if entries.len() < MAX_VISIBLE_ROWS {
            entries.push(entry);
        }
    }
    if peers.len() != count {
        return Err(String::from(
            "Romestead has not returned every counted player.",
        ));
    }
    if lines
        .get(start + count + 1)
        .is_some_and(|line| line.contains(") - ") && line.contains(" - Peer "))
    {
        return Err(String::from(
            "Romestead returned more players than its count header.",
        ));
    }
    Ok(finish(entries, count, None))
}

fn decimal_id(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(String::from("Console player ID is malformed."));
    }
    value
        .parse()
        .map_err(|_| String::from("Console player ID is out of range."))
}

fn terraria(lines: &[&str], request_id: &str) -> Result<ParsedPlayerList, String> {
    let (end, count) = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| terraria_count(terraria_line(line)).map(|count| (index, count)))
        .ok_or("Terraria has not returned its connected-player count footer.")?;
    let count = count?;
    if count > 255 || end < count {
        return Err(String::from(
            "Terraria player count or response length is invalid.",
        ));
    }
    if end > count
        && terraria_line(lines[end - count - 1]).ends_with(')')
        && terraria_line(lines[end - count - 1]).contains(" (")
    {
        return Err(String::from(
            "Terraria returned more player rows than its count footer.",
        ));
    }
    let mut entries = Vec::new();
    for line in &lines[end - count..end] {
        let row = terraria_line(line)
            .strip_suffix(')')
            .ok_or("Terraria player row is incomplete.")?;
        let (name, address) = row
            .rsplit_once(" (")
            .ok_or("Terraria player address boundary is missing.")?;
        if address.trim().is_empty() || address.chars().any(char::is_control) {
            return Err(String::from("Terraria player address is malformed."));
        }
        entries.push(display_player(name, request_id, entries.len() + 1, None)?);
    }
    Ok(finish(entries, count, None))
}

fn terraria_line(line: &str) -> &str {
    // The dedicated input loop prints this prompt without a newline before Console.ReadLine.
    line.strip_prefix(": ").unwrap_or(line)
}

fn terraria_count(line: &str) -> Option<Result<usize, String>> {
    // CLI strings from all twelve shipped 1.4.5.6 language resources. English commands
    // are accepted in every language, but the response remains localized.
    const EMPTY: &[&str] = &[
        "No players connected.",
        "Keine Spieler verbunden.",
        "No hay jugadores conectados.",
        "Aucun joueur connecté.",
        "Nessun giocatore connesso.",
        "Nie przyłączyli się żadni gracze.",
        "Nenhum jogador conectado.",
        "Нет подключенных игроков.",
        "无玩家连接。",
        "接続中のプレイヤーはいません。",
        "접속 중인 플레이어가 없음.",
        "沒有玩家連線。",
    ];
    const SINGULAR: &[&str] = &[
        "1 player connected.",
        "1 Spieler verbunden.",
        "1 jugador conectado.",
        "1 joueur connecté.",
        "1 giocatore connesso.",
        "Przyłączył się 1 gracz.",
        "1 jogador conectado.",
        "Подключён 1 игрок.",
        "1个玩家已连接。",
        "1人のプレイヤーが接続中です。",
        "플레이어 1명 접속 중.",
        "已與 1 名玩家連線。",
    ];
    const COUNTED: &[(&str, &str)] = &[
        ("", " players connected."),
        ("", " Spieler verbunden."),
        ("", " jugadores conectados."),
        ("", " joueurs connectés."),
        ("", " giocatori connessi."),
        ("Przyłączyła się następująca liczba graczy: ", "."),
        ("", " jogadores conectados."),
        ("Подключено игроков: ", "."),
        ("", "个玩家已连接。"),
        ("", "人のプレイヤーが接続中です。"),
        ("플레이어 ", "명 접속 중."),
        ("已與 ", " 名玩家連線。"),
    ];
    if EMPTY.contains(&line) {
        return Some(Ok(0));
    }
    if SINGULAR.contains(&line) {
        return Some(Ok(1));
    }
    COUNTED.iter().find_map(|(prefix, suffix)| {
        line.strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .map(count_value)
    })
}

fn necesse_line(line: &str) -> &str {
    // GameLog's ordinary stdout uses FormatPrefix.WHITE (ANSI 39), then this timestamp.
    let line = line.strip_prefix("\u{1b}[39m").unwrap_or(line);
    let prefix = line.as_bytes();
    if prefix.len() >= 22
        && prefix[0] == b'['
        && prefix[20] == b']'
        && prefix[21] == b' '
        && prefix[1..20]
            .iter()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b' ',
                13 | 16 => *byte == b':',
                _ => byte.is_ascii_digit(),
            })
    {
        &line[22..]
    } else {
        line
    }
}

fn count_value(value: &str) -> Result<usize, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(String::from(
            "Console player count or slot is not a decimal number.",
        ));
    }
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value <= MAX_ROWS)
        .ok_or_else(|| String::from("Console player count or slot exceeds the row limit."))
}

fn display_player(
    name: &str,
    request_id: &str,
    index: usize,
    ping_ms: Option<u32>,
) -> Result<RuntimeLivePlayerEntry, String> {
    if name.trim().is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
        return Err(String::from(
            "Console player response contains an invalid name.",
        ));
    }
    Ok(RuntimeLivePlayerEntry {
        player_key: format!("console:{request_id}:{index}"),
        display_name: name.to_owned(),
        identifiers: Vec::new(),
        available_action_ids: Vec::new(),
        ping_ms,
        session_started_at_unix_ms: None,
        role: None,
        attributes: Vec::new(),
    })
}

fn finish(
    entries: Vec<RuntimeLivePlayerEntry>,
    count: usize,
    max_players: Option<usize>,
) -> ParsedPlayerList {
    let truncated = entries.len() != count;
    ParsedPlayerList {
        entries,
        bindings: HashMap::new(),
        complete: !truncated,
        truncated,
        current_players: Some(count),
        max_players,
    }
}
