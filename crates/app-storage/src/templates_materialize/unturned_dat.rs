//! Surgical updates for Unturned's line-oriented DAT configuration.
//! Unowned values and comments retain their exact bytes; malformed input fails closed.

use std::collections::{HashMap, HashSet};

#[derive(Debug)]
struct Node {
    key: String,
    start: usize,
    end: usize,
    children: Option<Vec<Node>>,
    close: usize,
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    start: usize,
    end: usize,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut i = if text.starts_with('\u{feff}') { 3 } else { 0 };
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        b'\n' | b'\r' => return Err("newline in quoted DAT value".into()),
                        _ => i += 1,
                    }
                }
                if !closed {
                    return Err("unterminated quoted DAT value".into());
                }
                result.push(Token {
                    text: text[start..i].into(),
                    start,
                    end: i,
                });
            }
            b'\n' | b'{' | b'}' | b'[' | b']' => {
                result.push(Token {
                    text: text[i..i + 1].into(),
                    start: i,
                    end: i + 1,
                });
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'\x0b'
                    && !matches!(bytes[i], b'{' | b'}' | b'[' | b']' | b'"')
                {
                    i += 1;
                }
                if i == start {
                    return Err("unexpected DAT token boundary".into());
                }
                result.push(Token {
                    text: text[start..i].into(),
                    start,
                    end: i,
                });
            }
        }
    }
    Ok(result)
}

fn parse_nodes(
    tokens: &[Token],
    cursor: &mut usize,
    nested: bool,
    depth: usize,
) -> Result<Vec<Node>, String> {
    if depth > 32 {
        return Err("DAT nesting exceeds 32 levels".into());
    }
    let mut nodes = Vec::new();
    let mut names = HashSet::new();
    while *cursor < tokens.len() {
        let key = &tokens[*cursor];
        if key.text == "\n" {
            *cursor += 1;
            continue;
        }
        if key.text == "}" && nested {
            return Ok(nodes);
        }
        if matches!(key.text.as_str(), "{" | "}" | "[" | "]") {
            return Err("expected DAT property name".into());
        }
        let name = key.text.trim_matches('"').to_owned();
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(format!("duplicate DAT property {name}"));
        }
        *cursor += 1;
        let mut node = Node {
            key: name,
            start: key.start,
            end: key.end,
            children: None,
            close: key.end,
        };
        let mut peek = *cursor;
        while tokens.get(peek).is_some_and(|token| token.text == "\n") {
            peek += 1;
        }
        if tokens.get(peek).is_some_and(|token| token.text == "{") {
            *cursor = peek + 1;
            node.children = Some(parse_nodes(tokens, cursor, true, depth + 1)?);
            let close = tokens.get(*cursor).ok_or("unclosed DAT dictionary")?;
            node.close = close.start;
            node.end = close.end;
            *cursor += 1;
        } else if tokens.get(peek).is_some_and(|token| token.text == "[") {
            *cursor = peek + 1;
            let mut stack = vec!["]"];
            while !stack.is_empty() {
                let token = tokens.get(*cursor).ok_or("unclosed DAT list")?;
                match token.text.as_str() {
                    "[" => stack.push("]"),
                    "{" => stack.push("}"),
                    "]" | "}" if stack.pop() != Some(token.text.as_str()) => {
                        return Err("mismatched DAT delimiter".into());
                    }
                    _ => {}
                }
                if stack.len() > 32 {
                    return Err("DAT nesting exceeds 32 levels".into());
                }
                node.end = token.end;
                *cursor += 1;
            }
        } else {
            while let Some(value) = tokens.get(*cursor) {
                if value.text == "\n" {
                    break;
                }
                if matches!(value.text.as_str(), "{" | "}" | "[" | "]") {
                    return Err("unexpected DAT delimiter in scalar value".into());
                }
                node.end = value.end;
                *cursor += 1;
            }
        }
        nodes.push(node);
    }
    if nested {
        return Err("unclosed DAT dictionary".into());
    }
    Ok(nodes)
}

fn indent(text: &str, offset: usize) -> &str {
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    &text[start..offset]
}

fn reindent(text: &str, node: &Node, target_indent: &str, newline: &str) -> String {
    let source_indent = indent(text, node.start);
    text[node.start..node.end]
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                line.to_owned()
            } else {
                format!(
                    "{target_indent}{}",
                    line.strip_prefix(source_indent).unwrap_or(line)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(newline)
}

fn plan_updates(
    original: &str,
    old: &[Node],
    rendered: &str,
    new: &[Node],
    close: usize,
    nested: bool,
    edits: &mut Vec<(usize, usize, String)>,
) -> Result<(), String> {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut additions = String::new();
    let target_indent = if nested {
        format!("{}\t", indent(original, close))
    } else {
        String::new()
    };
    let old_by_name: HashMap<_, _> = old
        .iter()
        .map(|node| (node.key.to_ascii_lowercase(), node))
        .collect();
    for new_node in new {
        if let Some(old_node) = old_by_name.get(&new_node.key.to_ascii_lowercase()) {
            if let (Some(old_children), Some(new_children)) =
                (&old_node.children, &new_node.children)
            {
                plan_updates(
                    original,
                    old_children,
                    rendered,
                    new_children,
                    old_node.close,
                    true,
                    edits,
                )?;
            } else if old_node.children.is_some() != new_node.children.is_some() {
                return Err(format!(
                    "native property {} has an incompatible dictionary value",
                    new_node.key
                ));
            } else {
                edits.push((
                    old_node.start,
                    old_node.end,
                    reindent(
                        rendered,
                        new_node,
                        indent(original, old_node.start),
                        newline,
                    ),
                ));
            }
        } else {
            additions.push_str(&target_indent);
            additions.push_str(&reindent(rendered, new_node, &target_indent, newline));
            additions.push_str(newline);
        }
    }
    if !additions.is_empty() {
        let offset = if nested {
            close - indent(original, close).len()
        } else {
            close
        };
        if offset > 0 && !original[..offset].ends_with('\n') {
            additions.insert_str(0, newline);
        }
        edits.push((offset, offset, additions));
    }
    Ok(())
}

fn scalar_token(token: &str) -> String {
    if token.starts_with('"') && token.ends_with('"') {
        let mut result = String::new();
        let mut chars = token[1..token.len() - 1].chars();
        while let Some(ch) = chars.next() {
            let decoded = if ch == '\\' {
                match chars.next() {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some(other @ ('\\' | '"')) => other,
                    Some(other) => {
                        result.push('\\');
                        other
                    }
                    None => '\\',
                }
            } else {
                ch
            };
            result.push(decoded);
        }
        return result;
    }
    if let Ok(number) = token.parse::<f64>() {
        return number.to_string();
    }
    if token.eq_ignore_ascii_case("true") || token.eq_ignore_ascii_case("false") {
        return token.to_ascii_lowercase();
    }
    token.into()
}

fn comments_between_tokens<'a>(text: &'a str, tokens: &[Token]) -> Vec<&'a str> {
    let mut previous_end = 0;
    let mut comments = Vec::new();
    for token in tokens {
        comments.extend(
            text[previous_end..token.start]
                .lines()
                .filter_map(|line| line.find("//").map(|start| &line[start..])),
        );
        previous_end = token.end;
    }
    comments
}

fn same_value(left: &str, old: &Node, right: &str, previous: &Node) -> Result<bool, String> {
    let left_text = &left[old.start..old.end];
    let right_text = &right[previous.start..previous.end];
    let left = tokens(left_text)?;
    let right = tokens(right_text)?;
    // Removing a collection also removes its internal comments. A changed comment
    // therefore releases the user's edited collection instead of deleting it.
    if comments_between_tokens(left_text, &left) != comments_between_tokens(right_text, &right) {
        return Ok(false);
    }
    Ok(left
        .iter()
        .skip(1)
        .filter(|t| t.text != "\n")
        .map(|t| scalar_token(&t.text))
        .eq(right
            .iter()
            .skip(1)
            .filter(|t| t.text != "\n")
            .map(|t| scalar_token(&t.text))))
}

fn plan_released_overrides(
    original: &str,
    old: &[Node],
    new: &[Node],
    previous_text: &str,
    previous: &[Node],
    allowed: &HashSet<String>,
    edits: &mut Vec<(usize, usize, String)>,
) -> Result<(), String> {
    let old_groups: HashMap<_, _> = old
        .iter()
        .map(|node| (node.key.to_ascii_lowercase(), node))
        .collect();
    let new_groups: HashMap<_, _> = new
        .iter()
        .map(|node| (node.key.to_ascii_lowercase(), node))
        .collect();
    for group in previous {
        let name = group.key.to_ascii_lowercase();
        let Some(fields) = &group.children else {
            continue;
        };
        let Some(old_fields) = old_groups
            .get(&name)
            .and_then(|node| node.children.as_ref())
        else {
            continue;
        };
        let old_fields: HashMap<_, _> = old_fields
            .iter()
            .map(|node| (node.key.to_ascii_lowercase(), node))
            .collect();
        let new_fields: HashSet<_> = new_groups
            .get(&name)
            .and_then(|node| node.children.as_ref())
            .into_iter()
            .flatten()
            .map(|node| node.key.to_ascii_lowercase())
            .collect();
        for field in fields {
            let field_name = field.key.to_ascii_lowercase();
            if !allowed.contains(&format!("{name}.{field_name}"))
                || new_fields.contains(&field_name)
            {
                continue;
            }
            if let Some(old_field) = old_fields.get(&field_name)
                && same_value(original, old_field, previous_text, field)?
            {
                // A cleared override only removes our unchanged value. External edits retain ownership.
                edits.push((old_field.start, old_field.end, String::new()));
            }
        }
    }
    Ok(())
}

pub(super) fn merge_managed(
    original: &str,
    rendered: &str,
    previous: &str,
    allowed: &HashSet<String>,
) -> Result<String, String> {
    if [original, rendered, previous]
        .iter()
        .any(|text| text.len() > 4 * 1024 * 1024)
    {
        return Err("DAT configuration exceeds 4 MiB".into());
    }
    let old = parse_nodes(&tokens(original)?, &mut 0, false, 0)?;
    let new = parse_nodes(&tokens(rendered)?, &mut 0, false, 0)?;
    let previous_nodes = parse_nodes(&tokens(previous)?, &mut 0, false, 0)?;
    let mut edits = Vec::new();
    plan_updates(
        original,
        &old,
        rendered,
        &new,
        original.len(),
        false,
        &mut edits,
    )?;
    plan_released_overrides(
        original,
        &old,
        &new,
        previous,
        &previous_nodes,
        allowed,
        &mut edits,
    )?;
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut result = original.to_owned();
    for (start, end, replacement) in edits {
        result.replace_range(start..end, &replacement);
    }
    if result.len() > 4 * 1024 * 1024 {
        return Err("merged DAT configuration exceeds 4 MiB".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(original: &str, rendered: &str) -> Result<String, String> {
        merge_managed(original, rendered, "", &HashSet::new())
    }

    #[test]
    fn clearing_releases_only_unchanged_owned_fields_and_preserves_comments() {
        let previous = "Version 1\nItems\n{\nSpawn_Chance 0.25\nHas_Durability false\n}\n";
        let old = "\u{feff}// note\r\nVersion 1\r\nItems\r\n{\r\nSpawn_Chance 0.250 // retained note\r\nHas_Durability true\r\nUnknown 9\r\n}\r\n";
        let allowed = ["items.spawn_chance".into(), "items.has_durability".into()].into();
        let result = merge_managed(old, "Version 1\n", previous, &allowed).unwrap();
        assert!(!result.contains("Spawn_Chance"));
        assert!(result.contains("// retained note\r\n"));
        assert!(result.contains("Has_Durability true\r\n"));
        assert!(result.contains("Unknown 9\r\n"));
        assert!(result.starts_with("\u{feff}// note"));
        assert_eq!(
            merge_managed(&result, "Version 1\n", "Version 1\n", &allowed).unwrap(),
            result
        );
        // Corrupt or future ownership metadata cannot remove unknown keys.
        assert_eq!(
            merge_managed(old, "Version 1\n", "Items\n{\nUnknown 9\n}\n", &allowed).unwrap(),
            old
        );
    }

    #[test]
    fn consumes_control_whitespace_without_stalling() {
        for whitespace in ['\u{000b}', '\u{000c}'] {
            let original = format!("\u{feff}Version 1\n{whitespace}");
            let parsed = tokens(&original).unwrap();
            assert_eq!(
                parsed
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>(),
                ["Version", "1", "\n"]
            );
            assert!(parsed.iter().all(|token| token.end > token.start));
            assert_eq!(merge(&original, "Version 1\n").unwrap(), original);
            let separated = format!("Version{whitespace}1\n");
            assert_eq!(
                tokens(&separated)
                    .unwrap()
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>(),
                ["Version", "1", "\n"]
            );
        }
    }

    #[test]
    fn clearing_links_preserves_added_internal_comments_and_releases_ownership() {
        let previous = "Browser\n{\n\tLinks\n\t[\n\t\t{\n\t\t\tMessage \"Rules // link\"\n\t\t\tURL \"https://example.test\"\n\t\t}\n\t]\n}\n";
        let allowed = ["browser.links".into()].into();
        let original = previous.replace("\t\t{\n", "\t\t// user note\n\t\t{\n");
        let result = merge_managed(&original, "", previous, &allowed).unwrap();
        assert_eq!(result, original);
        assert_eq!(merge_managed(&result, "", "", &allowed).unwrap(), original);
        // Quoted comment markers and URLs are data, while formatting alone does not
        // keep an unchanged override from returning to the game's native default.
        let reformatted = previous.replace('\t', "  ").replace('\n', "\r\n");
        assert!(
            !merge_managed(&reformatted, "", previous, &allowed)
                .unwrap()
                .contains("Links")
        );
        let old_comment = previous.replace("\t\t{\n", "\t\t// prior note\n\t\t{\n");
        assert_eq!(
            merge_managed(&original, "", &old_comment, &allowed).unwrap(),
            original
        );
    }

    #[test]
    fn preserves_unquoted_urls_and_rejects_scalar_dictionary_replacement() {
        let original = "Browser\n{\nThumbnail https://example.test/banner.png\n}\n";
        assert_eq!(
            merge(original, "Version 1\n").unwrap(),
            format!("{original}Version 1\n")
        );
        assert!(merge("Items invalid\n", "Items\n{\nSpawn_Chance 0.25\n}\n").is_err());
        let old = "Browser\n{\nDesc_Server_List \"\\q\"\n}\n";
        let previous = "Browser\n{\nDesc_Server_List \"q\"\n}\n";
        let allowed = ["browser.desc_server_list".into()].into();
        assert_eq!(merge_managed(old, "", previous, &allowed).unwrap(), old);
    }

    #[test]
    fn updates_native_blocks_and_preserves_unknown_values_comments_and_unset_overrides() {
        let old = "// user note\r\nVersion 1\r\nItems\r\n{\r\n\tSpawn_Chance 0.75 // custom\r\n\tFuture_Value 9\r\n}\r\nPlayers\r\n{\r\n\tExperience_Multiplier 3\r\n}\r\n";
        let new = "Version 1\nItems\n{\n\tSpawn_Chance 0.25\n\tHas_Durability false\n}\nGameplay\n{\n\tTimer_Exit 0\n}\n";
        let result = merge(old, new).unwrap();
        assert!(result.contains("Spawn_Chance 0.25 // custom\r\n"));
        assert!(result.contains("\tFuture_Value 9\r\n"));
        assert!(result.contains("\tExperience_Multiplier 3\r\n"));
        assert!(result.starts_with("// user note\r\n"));
        assert!(result.contains("Has_Durability false\r\n"));
        assert!(result.contains("Timer_Exit 0\r\n"));
        assert_eq!(merge(&result, new).unwrap(), result);
    }

    #[test]
    fn treats_quoted_delimiters_as_data_and_rejects_ambiguous_or_truncated_input() {
        let old = "Browser\n{\n\tLinks\n\t[\n\t\t{\n\t\t\tMessage \"keep } // text\"\n\t\t\tURL \"https://example.test\"\n\t\t}\n\t]\n}\n";
        let new = "Browser\n{\n\tThumbnail \"https://example.test/a.png\"\n}\n";
        assert!(
            merge(old, new)
                .unwrap()
                .contains("Message \"keep } // text\"")
        );
        for bad in [
            "Items\n{\nKey 1",
            "Items\n{\nKey 1\nkey 2\n}",
            "Key \"unterminated",
            "List\n[\n}\n",
        ] {
            assert!(merge(bad, new).is_err(), "{bad}");
        }
    }
}
