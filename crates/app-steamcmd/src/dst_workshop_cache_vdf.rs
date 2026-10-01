#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KvEntry {
    pub(crate) key: String,
    pub(crate) value: KvValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum KvValue {
    Text(String),
    Object(Vec<KvEntry>),
}

#[derive(Debug)]
enum Token {
    Text(String),
    Open,
    Close,
}

const MAX_OBJECT_DEPTH: usize = 64;

pub(crate) fn parse_document(text: &str) -> Result<Vec<KvEntry>, String> {
    let tokens = tokenize(text)?;
    let mut index = 0;
    let entries = parse_entries(&tokens, &mut index, false, 0)?;
    if index != tokens.len() {
        return Err(String::from("unexpected trailing token"));
    }
    Ok(entries)
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        match bytes[index] {
            b'{' => {
                tokens.push(Token::Open);
                index += 1;
            }
            b'}' => {
                tokens.push(Token::Close);
                index += 1;
            }
            b'"' => {
                index += 1;
                let mut value = Vec::new();
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                        if index == bytes.len() {
                            return Err(String::from("unterminated escape"));
                        }
                    }
                    value.push(bytes[index]);
                    index += 1;
                }
                if index == bytes.len() {
                    return Err(String::from("unterminated quoted string"));
                }
                index += 1;
                tokens.push(Token::Text(
                    String::from_utf8(value).map_err(|_| String::from("invalid UTF-8 string"))?,
                ));
            }
            _ => return Err(format!("unexpected byte at offset {index}")),
        }
    }
    Ok(tokens)
}

fn parse_entries(
    tokens: &[Token],
    index: &mut usize,
    nested: bool,
    depth: usize,
) -> Result<Vec<KvEntry>, String> {
    let mut entries = Vec::new();
    loop {
        match tokens.get(*index) {
            Some(Token::Close) if nested => {
                *index += 1;
                return Ok(entries);
            }
            None if nested => return Err(String::from("unterminated object")),
            None => return Ok(entries),
            Some(Token::Text(_)) => {}
            _ => return Err(String::from("expected key")),
        }
        let Token::Text(key) = &tokens[*index] else {
            unreachable!()
        };
        *index += 1;
        let value = match tokens.get(*index) {
            Some(Token::Text(value)) => {
                *index += 1;
                KvValue::Text(value.clone())
            }
            Some(Token::Open) => {
                if depth >= MAX_OBJECT_DEPTH {
                    return Err(format!(
                        "object nesting exceeds the {MAX_OBJECT_DEPTH}-level limit"
                    ));
                }
                *index += 1;
                KvValue::Object(parse_entries(tokens, index, true, depth + 1)?)
            }
            _ => return Err(format!("missing value for {key}")),
        };
        entries.push(KvEntry {
            key: key.clone(),
            value,
        });
    }
}

pub(crate) fn render_document(document: &[KvEntry]) -> String {
    fn render(entries: &[KvEntry], depth: usize, output: &mut String) {
        for entry in entries {
            let indent = "\t".repeat(depth);
            output.push_str(&indent);
            output.push('"');
            output.push_str(&escape(&entry.key));
            output.push('"');
            match &entry.value {
                KvValue::Text(value) => {
                    output.push_str("\t\t\"");
                    output.push_str(&escape(value));
                    output.push_str("\"\n");
                }
                KvValue::Object(value) => {
                    output.push('\n');
                    output.push_str(&indent);
                    output.push_str("{\n");
                    render(value, depth + 1, output);
                    output.push_str(&indent);
                    output.push_str("}\n");
                }
            }
        }
    }
    let mut output = String::new();
    render(document, 0, &mut output);
    output
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
