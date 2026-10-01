use std::fs;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;

const MAX_SHARDINDEX_BYTES: u64 = 1024 * 1024;
const MAX_SHARDINDEX_DEPTH: usize = 64;
const MAX_SESSION_ID_BYTES: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DstShardSaveInspection {
    Valid,
    Invalid(String),
}

pub(super) fn inspect_dst_shard_save(
    save_root: &Path,
    entry_budget: &mut usize,
) -> Result<DstShardSaveInspection, String> {
    let shardindex = save_root.join("shardindex");
    let Some(metadata) = inspect_path(&shardindex)? else {
        return Ok(invalid("shardindex is missing"));
    };
    if is_link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() == 0 {
        return Ok(invalid("shardindex is not a non-empty plain file"));
    }
    if metadata.len() > MAX_SHARDINDEX_BYTES {
        return Ok(invalid("shardindex exceeds the 1 MiB validation limit"));
    }
    let mut bytes = Vec::new();
    fs::File::open(&shardindex)
        .map_err(|error| {
            format!(
                "Failed to open DST shardindex {}: {error}",
                shardindex.display()
            )
        })?
        .take(MAX_SHARDINDEX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!(
                "Failed to read DST shardindex {}: {error}",
                shardindex.display()
            )
        })?;
    if bytes.len() as u64 > MAX_SHARDINDEX_BYTES {
        return Ok(invalid("shardindex exceeds the 1 MiB validation limit"));
    }
    let session_id = match parse_top_level_session_id(&bytes) {
        Ok(session_id) => session_id,
        Err(reason) => return Ok(invalid(reason)),
    };

    let session_root = save_root.join("session");
    let Some(session_root_metadata) = inspect_path(&session_root)? else {
        return Ok(invalid("session directory is missing"));
    };
    if is_link_or_reparse(&session_root_metadata) || !session_root_metadata.is_dir() {
        return Ok(invalid("session root is not a plain directory"));
    }
    let session_path = session_root.join(&session_id);
    let Some(session_metadata) = inspect_path(&session_path)? else {
        return Ok(invalid(format!(
            "shardindex references session {session_id}, but its snapshot directory does not exist"
        )));
    };
    if is_link_or_reparse(&session_metadata) || !session_metadata.is_dir() {
        return Ok(invalid(format!(
            "shardindex session {session_id} is not a plain directory"
        )));
    }

    let entries = fs::read_dir(&session_path).map_err(|error| {
        format!(
            "Failed to read referenced DST session {}: {error}",
            session_path.display()
        )
    })?;
    for entry in entries {
        if *entry_budget == 0 {
            return Ok(invalid("referenced session exceeds the scan entry limit"));
        }
        *entry_budget -= 1;
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect referenced DST session {}: {error}",
                session_path.display()
            )
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.is_empty() || name.len() > 32 || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if !is_nonempty_plain_file(&entry.path())? {
            continue;
        }
        if is_nonempty_plain_file(&session_path.join(format!("{name}.meta")))? {
            return Ok(DstShardSaveInspection::Valid);
        }
    }

    Ok(invalid(format!(
        "shardindex session {session_id} has no numeric snapshot with a non-empty .meta pair"
    )))
}

fn invalid(reason: impl Into<String>) -> DstShardSaveInspection {
    DstShardSaveInspection::Invalid(reason.into())
}

fn inspect_path(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Failed to inspect DST save path {}: {error}",
            path.display()
        )),
    }
}

fn is_nonempty_plain_file(path: &Path) -> Result<bool, String> {
    Ok(inspect_path(path)?.is_some_and(|metadata| {
        metadata.is_file() && metadata.len() > 0 && !is_link_or_reparse(&metadata)
    }))
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Ident(Vec<u8>),
    String { value: Vec<u8>, escaped: bool },
    LeftBrace,
    RightBrace,
    Equals,
    Other,
}

fn parse_top_level_session_id(bytes: &[u8]) -> Result<String, &'static str> {
    let mut lexer = Lexer::new(bytes);
    let mut saw_return = false;
    let mut depth = 0usize;
    let mut session_id = None;

    while let Some(token) = lexer.next_token()? {
        if depth == 0 {
            match token {
                Token::Ident(value) if value == b"return" => saw_return = true,
                Token::LeftBrace if saw_return => depth = 1,
                _ => {}
            }
            continue;
        }

        match token {
            Token::LeftBrace => {
                depth += 1;
                if depth > MAX_SHARDINDEX_DEPTH {
                    return Err("shardindex exceeds the nesting depth limit");
                }
            }
            Token::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Token::Ident(value) if depth == 1 && value == b"session_id" => {
                if session_id.is_some() {
                    return Err("shardindex contains duplicate top-level session_id fields");
                }
                if lexer.next_token()? != Some(Token::Equals) {
                    return Err("shardindex top-level session_id is not an assignment");
                }
                let Some(Token::String { value, escaped }) = lexer.next_token()? else {
                    return Err("shardindex top-level session_id is not a quoted string");
                };
                if escaped {
                    return Err("shardindex session_id must not contain escape sequences");
                }
                if value.is_empty()
                    || value.len() > MAX_SESSION_ID_BYTES
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err("shardindex session_id is not a safe session directory name");
                }
                session_id = Some(
                    String::from_utf8(value)
                        .map_err(|_| "shardindex session_id is not valid UTF-8")?,
                );
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err("shardindex return table is incomplete");
    }
    session_id.ok_or("shardindex has no top-level session_id")
}

struct Lexer<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn next_token(&mut self) -> Result<Option<Token>, &'static str> {
        self.skip_trivia()?;
        let Some(&byte) = self.bytes.get(self.position) else {
            return Ok(None);
        };
        self.position += 1;
        Ok(Some(match byte {
            b'{' => Token::LeftBrace,
            b'}' => Token::RightBrace,
            b'=' => Token::Equals,
            b'\'' | b'"' => self.read_string(byte)?,
            b'[' => {
                self.position -= 1;
                if !self.skip_long_bracket()? {
                    self.position += 1;
                }
                Token::Other
            }
            value if is_ident_start(value) => self.read_ident(value),
            _ => Token::Other,
        }))
    }

    fn skip_trivia(&mut self) -> Result<(), &'static str> {
        loop {
            while self
                .bytes
                .get(self.position)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                self.position += 1;
            }
            if self.bytes.get(self.position..self.position + 2) != Some(b"--") {
                return Ok(());
            }
            self.position += 2;
            if !self.skip_long_bracket()? {
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
                {
                    self.position += 1;
                }
            }
        }
    }

    fn skip_long_bracket(&mut self) -> Result<bool, &'static str> {
        if self.bytes.get(self.position) != Some(&b'[') {
            return Ok(false);
        }
        let mut cursor = self.position + 1;
        while self.bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            if cursor - self.position > 17 {
                return Err("shardindex long-string delimiter exceeds the validation limit");
            }
        }
        if self.bytes.get(cursor) != Some(&b'[') {
            return Ok(false);
        }
        let equals = cursor - self.position - 1;
        cursor += 1;
        let closing_len = equals + 2;
        let mut end = cursor;
        while end + closing_len <= self.bytes.len() {
            if self.bytes[end] == b']'
                && self.bytes[end + 1..end + 1 + equals]
                    .iter()
                    .all(|byte| *byte == b'=')
                && self.bytes[end + 1 + equals] == b']'
            {
                self.position = end + closing_len;
                return Ok(true);
            }
            end += 1;
        }
        Err("shardindex has an unterminated long string or block comment")
    }

    fn read_ident(&mut self, first: u8) -> Token {
        let mut value = vec![first];
        while let Some(&byte) = self.bytes.get(self.position) {
            if !is_ident_continue(byte) {
                break;
            }
            value.push(byte);
            self.position += 1;
        }
        Token::Ident(value)
    }

    fn read_string(&mut self, quote: u8) -> Result<Token, &'static str> {
        let mut value = Vec::new();
        let mut escaped = false;
        while let Some(&byte) = self.bytes.get(self.position) {
            self.position += 1;
            if byte == quote {
                return Ok(Token::String { value, escaped });
            }
            if byte == b'\\' {
                escaped = true;
                let Some(&escaped_byte) = self.bytes.get(self.position) else {
                    return Err("shardindex has an unterminated string escape");
                };
                self.position += 1;
                value.push(escaped_byte);
            } else {
                value.push(byte);
            }
        }
        Err("shardindex has an unterminated string")
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
#[path = "dst_save_validation_tests.rs"]
mod tests;
