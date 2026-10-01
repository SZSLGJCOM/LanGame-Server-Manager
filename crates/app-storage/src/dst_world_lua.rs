use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use serde_json::Value;

#[path = "dst_world_lua_strings.rs"]
mod strings;

const MAX_BYTES: usize = 128 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 8192;

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

pub(super) struct Node {
    pub span: Range<usize>,
    pub value: Literal,
}

pub(super) enum Literal {
    Scalar(Value),
    Table(Table),
}

pub(super) struct Entry {
    pub key: Option<String>,
    pub node: Node,
}

pub(super) struct Table {
    pub entries: Vec<Entry>,
    end: usize,
    separated: bool,
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries
            .iter()
            .find(|entry| entry.key.as_deref() == Some(key))
            .map(|entry| &entry.node)
    }
}

impl Node {
    pub fn table(&self) -> Option<&Table> {
        match &self.value {
            Literal::Table(table) => Some(table),
            Literal::Scalar(_) => None,
        }
    }

    pub fn scalar(&self) -> Option<&Value> {
        match &self.value {
            Literal::Scalar(value) => Some(value),
            Literal::Table(_) => None,
        }
    }
}

/// Parse data constructors only. Unsupported Lua remains an opaque user-owned script.
pub(super) fn parse(source: &str, fragment: bool) -> Option<Table> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let mut parser = Parser {
        source,
        position: 0,
        entries: 0,
    };
    parser.space()?;
    let table = if fragment {
        parser.fields(false, 1)?
    } else {
        if parser.identifier()? != "return" {
            return None;
        }
        parser.space()?;
        parser.take(b'{')?;
        parser.fields(true, 1)?
    };
    parser.space()?;
    if !fragment && parser.peek() == Some(b';') {
        parser.position += 1;
        parser.space()?;
    }
    (parser.position == source.len()).then_some(table)
}

/// Apply nonoverlapping value edits without serializing unknown data or comments.
pub(super) fn set_fields(
    source: &str,
    table: &Table,
    updates: &BTreeMap<String, String>,
) -> String {
    let mut edits = Vec::<(Range<usize>, String)>::new();
    let mut inserted = String::new();
    for (key, value) in updates {
        if let Some(node) = table.get(key) {
            edits.push((node.span.clone(), value.clone()));
        } else {
            inserted.push_str(&format!("\n  {key} = {value},"));
        }
    }
    if !inserted.is_empty() {
        inserted.push('\n');
        if !table.separated
            && let Some(last) = table.entries.last()
        {
            if last.node.span.end == table.end {
                inserted.insert(0, ',');
            } else {
                edits.push((last.node.span.end..last.node.span.end, String::from(",")));
            }
        }
        edits.push((table.end..table.end, inserted));
    }
    edits.sort_by(|left, right| {
        right
            .0
            .start
            .cmp(&left.0.start)
            .then(right.0.end.cmp(&left.0.end))
    });
    let mut result = source.to_string();
    for (span, value) in edits {
        result.replace_range(span, &value);
    }
    result
}

pub(super) fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character < ' ' => {
                result.push_str(&format!("\\{:03}", u32::from(character)))
            }
            character => result.push(character),
        }
    }
    result.push('"');
    result
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
    entries: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }

    fn take(&mut self, expected: u8) -> Option<()> {
        if self.peek()? != expected {
            return None;
        }
        self.position += 1;
        Some(())
    }

    fn space(&mut self) -> Option<()> {
        loop {
            while self.peek().is_some_and(is_space) {
                self.position += 1;
            }
            if !self.source[self.position..].starts_with("--") {
                return Some(());
            }
            self.position += 2;
            if strings::long_open(self.source, self.position).is_some() {
                strings::long_string(self.source, &mut self.position)?;
            } else {
                while self
                    .peek()
                    .is_some_and(|byte| byte != b'\n' && byte != b'\r')
                {
                    self.position += 1;
                }
            }
        }
    }

    fn identifier(&mut self) -> Option<&str> {
        let start = self.position;
        if !self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            return None;
        }
        self.position += 1;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.position += 1;
        }
        Some(&self.source[start..self.position])
    }

    fn fields(&mut self, braced: bool, depth: usize) -> Option<Table> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut entries = Vec::new();
        let mut keys = HashSet::new();
        let mut array_index = 0_u32;
        let mut separated = true;
        loop {
            self.space()?;
            if (braced && self.peek() == Some(b'}')) || (!braced && self.peek().is_none()) {
                let end = self.position;
                if braced {
                    self.position += 1;
                }
                return Some(Table {
                    entries,
                    end,
                    separated,
                });
            }
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return None;
            }
            let start = self.position;
            let (key, identity) = if self.peek() == Some(b'[')
                && strings::long_open(self.source, self.position).is_none()
            {
                self.position += 1;
                self.space()?;
                let node = self.value(depth)?;
                self.space()?;
                self.take(b']')?;
                self.space()?;
                self.take(b'=')?;
                match node.scalar()? {
                    Value::String(key) => (Some(key.clone()), format!("s:{key}")),
                    Value::Number(number) => (None, format!("n:{}", number.as_f64()? + 0.0)),
                    _ => return None,
                }
            } else {
                let identifier = self.identifier().map(str::to_owned);
                self.space()?;
                if let Some(key) = identifier.filter(|_| self.peek() == Some(b'=')) {
                    if is_keyword(&key) {
                        return None;
                    }
                    self.position += 1;
                    let identity = format!("s:{key}");
                    (Some(key), identity)
                } else {
                    self.position = start;
                    array_index += 1;
                    (None, format!("n:{array_index}"))
                }
            };
            if !keys.insert(identity) {
                return None;
            }
            self.space()?;
            let node = self.value(depth)?;
            entries.push(Entry { key, node });
            self.space()?;
            separated = matches!(self.peek(), Some(b',' | b';'));
            if separated {
                self.position += 1;
            } else if (braced && self.peek() != Some(b'}')) || (!braced && self.peek().is_some()) {
                return None;
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Node> {
        let start = self.position;
        let value = match self.peek()? {
            b'{' => {
                self.position += 1;
                Literal::Table(self.fields(true, depth + 1)?)
            }
            b'\'' | b'"' => Literal::Scalar(Value::String(strings::quoted(
                self.source,
                &mut self.position,
            )?)),
            b'[' => Literal::Scalar(Value::String(strings::long_string(
                self.source,
                &mut self.position,
            )?)),
            b'-' | b'.' | b'0'..=b'9' => Literal::Scalar(Value::Number(self.number()?)),
            _ => Literal::Scalar(match self.identifier()? {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "nil" => Value::Null,
                _ => return None,
            }),
        };
        Some(Node {
            span: start..self.position,
            value,
        })
    }

    fn number(&mut self) -> Option<serde_json::Number> {
        let negative = self.peek() == Some(b'-');
        if negative {
            self.position += 1;
            self.space()?;
        }
        let start = self.position;
        let mut digits = 0;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if exponent_start == self.position {
                return None;
            }
        }
        let value: f64 = self.source[start..self.position].parse().ok()?;
        serde_json::Number::from_f64(if negative { -value } else { value })
    }
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "and"
            | "break"
            | "do"
            | "else"
            | "elseif"
            | "end"
            | "false"
            | "for"
            | "function"
            | "goto"
            | "if"
            | "in"
            | "local"
            | "nil"
            | "not"
            | "or"
            | "repeat"
            | "return"
            | "then"
            | "true"
            | "until"
            | "while"
    )
}

#[cfg(test)]
#[path = "dst_world_lua_tests.rs"]
mod tests;
