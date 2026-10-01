//! Render StoreBrowse's public BBCode without accepting publisher-supplied HTML.
//!
//! Steam uses both URL-body images and attributed images whose MP4/WebM fields
//! describe looping animations. Media URLs stay on their original official host;
//! the existing frontend sanitizer and locale-aware media loader remain in charge.

const MAX_INPUT_BYTES: usize = 512 * 1024;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOKENS: usize = 32_768;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paragraph,
    H1,
    H2,
    H3,
    H4,
    Bold,
    Italic,
    Underline,
    Quote,
    List,
    OrderedList,
    Item,
    Link,
    Image,
    Break,
    Rule,
}

#[derive(Clone, Copy)]
struct Tag<'a> {
    kind: Kind,
    closing: bool,
    args: &'a str,
}

struct Token<'a> {
    raw: &'a str,
    tag: Option<Tag<'a>>,
    start: usize,
    end: usize,
}

enum Node<'a> {
    Text(&'a str),
    Element {
        tag: Tag<'a>,
        body: &'a str,
        raw: &'a str,
        children: Vec<Node<'a>>,
    },
}

pub(super) fn render(bbcode: &str, app_id: u64) -> Result<Option<String>, String> {
    if app_id == 0 {
        return Err("Steam description requires a positive app ID".into());
    }
    if bbcode.len() > MAX_INPUT_BYTES {
        return Err("Steam description exceeds its byte limit".into());
    }
    if bbcode.trim().is_empty() {
        return Ok(None);
    }
    let tokens = tokenize(bbcode)?;
    let nodes = parse_nodes(bbcode, &tokens, &mut 0, None, 0)?;
    let mut html = Html(String::with_capacity(bbcode.len()));
    render_nodes(&nodes, app_id, &mut html)?;
    Ok(Some(html.0))
}

fn parse_tag(raw: &str) -> Option<Tag<'_>> {
    let mut inner = raw.strip_prefix('[')?.strip_suffix(']')?.trim();
    let closing = inner.starts_with('/');
    if closing {
        inner = &inner[1..];
    }
    let end = inner
        .find(|ch: char| ch.is_ascii_whitespace() || ch == '=')
        .unwrap_or(inner.len());
    let kind = match inner[..end].to_ascii_lowercase().as_str() {
        "p" => Kind::Paragraph,
        "h1" => Kind::H1,
        "h2" => Kind::H2,
        "h3" => Kind::H3,
        "h4" => Kind::H4,
        "b" => Kind::Bold,
        "i" => Kind::Italic,
        "u" => Kind::Underline,
        "quote" => Kind::Quote,
        "list" => Kind::List,
        "olist" => Kind::OrderedList,
        "*" => Kind::Item,
        "url" => Kind::Link,
        "img" => Kind::Image,
        "br" => Kind::Break,
        "hr" => Kind::Rule,
        _ => return None,
    };
    let args = inner[end..].trim();
    if closing && !args.is_empty() {
        return None;
    }
    Some(Tag {
        kind,
        closing,
        args,
    })
}

fn tokenize(source: &str) -> Result<Vec<Token<'_>>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let start = cursor;
        let tag = if bytes[cursor] == b'[' {
            cursor += 1;
            let mut quote = None;
            let mut expecting_value = false;
            let mut unquoted_value = false;
            let mut closed = false;
            while cursor < source.len() {
                let byte = bytes[cursor];
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() {
                    if byte == b']' {
                        cursor += 1;
                        closed = true;
                        break;
                    }
                    if byte == b'[' {
                        break;
                    }
                    if expecting_value && !byte.is_ascii_whitespace() {
                        expecting_value = false;
                        if byte == b'\'' || byte == b'"' {
                            quote = Some(byte);
                        } else {
                            unquoted_value = true;
                        }
                    } else if unquoted_value {
                        if byte.is_ascii_whitespace() {
                            unquoted_value = false;
                        }
                    } else if byte == b'=' {
                        expecting_value = true;
                    }
                }
                cursor += 1;
            }
            if closed {
                parse_tag(&source[start..cursor])
            } else {
                None
            }
        } else {
            cursor += source[cursor..].find('[').unwrap_or(source.len() - cursor);
            None
        };
        tokens.push(Token {
            raw: &source[start..cursor],
            tag,
            start,
            end: cursor,
        });
        if tokens.len() > MAX_TOKENS {
            return Err("Steam description exceeds its token limit".into());
        }
    }
    Ok(tokens)
}

// A bounded descent consumes each token once. The only implicit close supported
// is a list item: older Steam descriptions may omit [/*] before the next item.
fn parse_nodes<'a>(
    source: &'a str,
    tokens: &[Token<'a>],
    cursor: &mut usize,
    stop: Option<Kind>,
    depth: usize,
) -> Result<Vec<Node<'a>>, String> {
    let mut nodes = Vec::new();
    while let Some(token) = tokens.get(*cursor) {
        if let Some(tag) = token.tag {
            if tag.closing && stop == Some(tag.kind) {
                break;
            }
            if stop == Some(Kind::Item)
                && ((!tag.closing && tag.kind == Kind::Item)
                    || (tag.closing && matches!(tag.kind, Kind::List | Kind::OrderedList)))
            {
                break;
            }
            if !tag.closing {
                *cursor += 1;
                if depth >= MAX_DEPTH {
                    return Err("Steam description exceeds its nesting limit".into());
                }
                let children = if matches!(tag.kind, Kind::Break | Kind::Rule) {
                    Vec::new()
                } else {
                    parse_nodes(source, tokens, cursor, Some(tag.kind), depth + 1)?
                };
                let body_end = tokens.get(*cursor).map_or(source.len(), |next| next.start);
                let mut end = body_end;
                if let Some(next) = tokens.get(*cursor)
                    && next
                        .tag
                        .is_some_and(|close| close.closing && close.kind == tag.kind)
                {
                    end = next.end;
                    *cursor += 1;
                }
                nodes.push(Node::Element {
                    tag,
                    body: &source[token.end..body_end],
                    raw: &source[token.start..end],
                    children,
                });
                continue;
            }
        }
        nodes.push(Node::Text(token.raw));
        *cursor += 1;
    }
    Ok(nodes)
}

struct Html(String);
impl Html {
    fn push(&mut self, value: &str) -> Result<(), String> {
        if self.0.len().saturating_add(value.len()) > MAX_OUTPUT_BYTES {
            return Err("Steam description exceeds its rendered byte limit".into());
        }
        self.0.push_str(value);
        Ok(())
    }

    fn text(&mut self, value: &str, line_breaks: bool) -> Result<(), String> {
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '&' => self.push("&amp;")?,
                '<' => self.push("&lt;")?,
                '>' => self.push("&gt;")?,
                '"' => self.push("&quot;")?,
                '\'' => self.push("&#39;")?,
                '\r' | '\n' if line_breaks => {
                    if ch == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    self.push("<br />")?;
                }
                _ => self.push(ch.encode_utf8(&mut [0; 4]))?,
            }
        }
        Ok(())
    }

    fn attribute(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.push(" ")?;
        self.push(name)?;
        self.push("=\"")?;
        self.text(value, false)?;
        self.push("\"")
    }
}

fn render_nodes(nodes: &[Node<'_>], app_id: u64, html: &mut Html) -> Result<(), String> {
    for node in nodes {
        let Node::Element {
            tag,
            body,
            raw,
            children,
        } = node
        else {
            if let Node::Text(text) = node {
                html.text(text, true)?;
            }
            continue;
        };
        if tag.kind == Kind::Image {
            if !render_image(tag.args, body, app_id, html)? {
                html.text(raw, true)?;
            }
            continue;
        }
        if tag.kind == Kind::Link {
            let candidate = tag
                .args
                .strip_prefix('=')
                .unwrap_or(body)
                .trim()
                .trim_matches(['\'', '"']);
            if let Some(url) = safe_url(candidate, app_id) {
                html.push("<a")?;
                html.attribute("href", &url)?;
                html.push(" target=\"_blank\" rel=\"noreferrer\">")?;
                render_nodes(children, app_id, html)?;
                html.push("</a>")?;
            } else {
                render_nodes(children, app_id, html)?;
            }
            continue;
        }
        if tag.kind == Kind::Break {
            html.push("<br />")?;
            continue;
        }
        if tag.kind == Kind::Rule {
            html.push("<hr />")?;
            continue;
        }
        let name = match tag.kind {
            Kind::Paragraph => "p",
            Kind::H1 => "h1",
            Kind::H2 => "h2",
            Kind::H3 => "h3",
            Kind::H4 => "h4",
            Kind::Bold => "b",
            Kind::Italic => "i",
            Kind::Underline => "u",
            Kind::Quote => "blockquote",
            Kind::List if tag.args == "=1" => "ol",
            Kind::List => "ul",
            Kind::OrderedList => "ol",
            Kind::Item => "li",
            _ => unreachable!("media and void tags are handled above"),
        };
        html.push("<")?;
        html.push(name)?;
        html.push(">")?;
        render_nodes(children, app_id, html)?;
        html.push("</")?;
        html.push(name)?;
        html.push(">")?;
    }
    Ok(())
}

fn attributes(mut args: &str) -> Option<Vec<(&str, &str)>> {
    let mut result = Vec::new();
    while !args.trim().is_empty() {
        args = args.trim_start();
        let end = args
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(args.len());
        if end == 0 {
            return None;
        }
        let key = &args[..end];
        args = args[end..].trim_start().strip_prefix('=')?.trim_start();
        let first = args.chars().next()?;
        let value;
        if first == '"' || first == '\'' {
            args = &args[1..];
            let end = args.find(first)?;
            value = &args[..end];
            args = &args[end + 1..];
            if !args.is_empty() && !args.starts_with(char::is_whitespace) {
                return None;
            }
        } else {
            let end = args.find(char::is_whitespace).unwrap_or(args.len());
            value = &args[..end];
            args = &args[end..];
        }
        if result
            .iter()
            .any(|(seen, _): &(&str, &str)| seen.eq_ignore_ascii_case(key))
        {
            return None;
        }
        result.push((key, value));
    }
    Some(result)
}

fn safe_url(value: &str, app_id: u64) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let expanded = value.replace(
        "{STEAM_APP_IMAGE}",
        &format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{app_id}"),
    );
    let normalized = if expanded.starts_with("//") {
        format!("https:{expanded}")
    } else {
        expanded
    };
    let url = reqwest::Url::parse(&normalized).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.into())
}

fn render_image(args: &str, body: &str, app_id: u64, html: &mut Html) -> Result<bool, String> {
    let Some(attrs) = attributes(args) else {
        return Ok(false);
    };
    let get = |key: &str| {
        attrs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| *value)
    };
    let image = get("src")
        .or(get("avif"))
        .or_else(|| (!body.trim().is_empty()).then_some(body.trim()));
    let poster = get("poster")
        .or(image)
        .and_then(|value| safe_url(value, app_id));
    let sources = [("webm", "video/webm"), ("mp4", "video/mp4")]
        .into_iter()
        .filter_map(|(key, mime)| {
            get(key)
                .and_then(|value| safe_url(value, app_id))
                .map(|url| (url, mime))
        })
        .collect::<Vec<_>>();
    let image = image.and_then(|value| safe_url(value, app_id));
    if sources.is_empty() && image.is_none() {
        return Ok(false);
    }
    html.push(if sources.is_empty() {
        "<img"
    } else {
        "<video autoplay loop muted playsinline"
    })?;
    for key in ["width", "height"] {
        if let Some(value) = get(key).filter(|value| {
            value.len() <= 5
                && value.bytes().all(|ch| ch.is_ascii_digit())
                && value.parse::<u32>().is_ok_and(|dimension| dimension > 0)
        }) {
            html.attribute(key, value)?;
        }
    }
    if sources.is_empty() {
        if let Some(url) = image {
            html.attribute("src", &url)?;
        }
        if let Some(alt) = get("alt") {
            html.attribute("alt", alt)?;
        }
        html.push(" />")?;
    } else {
        if let Some(url) = poster {
            html.attribute("poster", &url)?;
        }
        html.push(">")?;
        for (url, mime) in sources {
            html.push("<source")?;
            html.attribute("src", &url)?;
            html.attribute("type", mime)?;
            html.push(" />")?;
        }
        html.push("</video>")?;
    }
    // Attributed media normally has an empty body. Preserve any unexpected
    // accompanying copy instead of silently discarding publisher content.
    if !args.is_empty() && !body.trim().is_empty() {
        html.text(body, true)?;
    }
    Ok(true)
}

#[cfg(test)]
#[path = "steam_store_description_tests.rs"]
mod tests;
