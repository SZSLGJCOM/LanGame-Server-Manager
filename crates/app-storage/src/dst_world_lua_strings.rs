pub(super) fn long_open(source: &str, position: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(position) != Some(&b'[') {
        return None;
    }
    let mut cursor = position + 1;
    while bytes.get(cursor) == Some(&b'=') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'[')).then_some((cursor - position - 1, cursor + 1))
}

pub(super) fn long_string(source: &str, position: &mut usize) -> Option<String> {
    let (equals, start) = long_open(source, *position)?;
    let close = format!("]{}]", "=".repeat(equals));
    let end = start + source[start..].find(&close)?;
    *position = end + close.len();
    let mut characters = source[start..end].chars().peekable();
    let mut value = String::new();
    while let Some(character) = characters.next() {
        if matches!(character, '\n' | '\r') {
            if characters
                .peek()
                .is_some_and(|next| matches!(next, '\n' | '\r') && *next != character)
            {
                characters.next();
            }
            value.push('\n');
        } else {
            value.push(character);
        }
    }
    Some(value.strip_prefix('\n').unwrap_or(&value).to_string())
}

pub(super) fn quoted(source: &str, position: &mut usize) -> Option<String> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(*position)?;
    *position += 1;
    let mut result = Vec::new();
    loop {
        let byte = *bytes.get(*position)?;
        *position += 1;
        match byte {
            byte if byte == quote => return String::from_utf8(result).ok(),
            b'\r' | b'\n' => return None,
            b'\\' => {
                let escape = *bytes.get(*position)?;
                *position += 1;
                match escape {
                    b'a' => result.push(7),
                    b'b' => result.push(8),
                    b'f' => result.push(12),
                    b'n' => result.push(b'\n'),
                    b'r' => result.push(b'\r'),
                    b't' => result.push(b'\t'),
                    b'v' => result.push(11),
                    b'\\' | b'\'' | b'"' => result.push(escape),
                    b'\n' | b'\r' => {
                        if bytes
                            .get(*position)
                            .is_some_and(|next| matches!(next, b'\r' | b'\n') && *next != escape)
                        {
                            *position += 1;
                        }
                        result.push(b'\n');
                    }
                    b'z' => {
                        while bytes
                            .get(*position)
                            .is_some_and(|byte| super::is_space(*byte))
                        {
                            *position += 1;
                        }
                    }
                    b'x' => {
                        let digits = source.get(*position..*position + 2)?;
                        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            return None;
                        }
                        let value = u8::from_str_radix(digits, 16).ok()?;
                        *position += 2;
                        result.push(value);
                    }
                    b'0'..=b'9' => {
                        let mut value = u16::from(escape - b'0');
                        for _ in 0..2 {
                            let Some(next) =
                                bytes.get(*position).filter(|byte| byte.is_ascii_digit())
                            else {
                                break;
                            };
                            value = value * 10 + u16::from(*next - b'0');
                            *position += 1;
                        }
                        result.push(u8::try_from(value).ok()?);
                    }
                    _ => return None,
                }
            }
            byte => result.push(byte),
        }
    }
}
