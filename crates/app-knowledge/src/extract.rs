use dom_query::Document;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sources::Source;
use crate::{KnowledgeError, Result};

pub(crate) const MAX_TEXT: usize = 2 * 1024 * 1024;
pub(crate) const CHUNK_CHARS: usize = 1400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Extracted {
    pub title: String,
    pub body: String,
    pub links: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Chunk {
    pub heading: String,
    pub body: String,
    pub offset: usize,
    pub vector: Vec<f32>,
}

#[cfg(test)]
pub(crate) fn extract(
    source: &Source,
    url: &Url,
    content_type: &str,
    bytes: &[u8],
) -> Result<Extracted> {
    extract_with_context(
        source,
        url,
        content_type,
        bytes,
        &std::sync::atomic::AtomicBool::new(false),
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    )
}

pub(crate) fn extract_with_context(
    source: &Source,
    url: &Url,
    content_type: &str,
    bytes: &[u8],
    cancel: &std::sync::atomic::AtomicBool,
    deadline: std::time::Instant,
) -> Result<Extracted> {
    if crate::zendesk::is_article_url(source, url) {
        return crate::zendesk::extract_article(source, url, content_type, bytes, cancel, deadline);
    }
    let path = url.path().to_ascii_lowercase();
    let title_fallback = source.title.clone();
    let mut result = if content_type.contains("application/pdf") || path.ends_with(".pdf") {
        let body = crate::pdf_worker::extract(bytes, cancel, deadline)?;
        Extracted {
            title: title_fallback,
            body,
            links: Vec::new(),
        }
    } else if content_type.contains("text/plain") || path.ends_with(".md") || path.ends_with(".txt")
    {
        let body = std::str::from_utf8(bytes)
            .map_err(|_| KnowledgeError::Invalid(format!("Documentation is not UTF-8: {url}")))?
            .to_owned();
        let title = body
            .lines()
            .find_map(|line| line.strip_prefix("# "))
            .unwrap_or(&title_fallback)
            .trim()
            .to_owned();
        let links = markdown_links(url, &body);
        Extracted { title, body, links }
    } else if content_type.contains("html") || content_type.is_empty() {
        let html = std::str::from_utf8(bytes).map_err(|_| {
            KnowledgeError::Invalid(format!("Documentation HTML is not UTF-8: {url}"))
        })?;
        let doc = Document::from(html);
        let restricted = doc.select("meta[name][content]").iter().any(|meta| {
            meta.attr("name")
                .is_some_and(|name| name.eq_ignore_ascii_case("robots"))
                && meta.attr("content").is_some_and(|content| {
                    content
                        .split(|character: char| {
                            character == ',' || character.is_ascii_whitespace()
                        })
                        .any(|directive| {
                            matches!(
                                directive.to_ascii_lowercase().as_str(),
                                "noindex" | "noarchive" | "none"
                            )
                        })
                })
        });
        if restricted {
            return Err(KnowledgeError::Policy(format!(
                "Publisher metadata disallows indexing: {url}"
            )));
        }
        let title = doc.select("h1").text().trim().to_string();
        let title = if title.is_empty() {
            doc.select("title").text().trim().to_string()
        } else {
            title
        };
        let link_selection = if let Some(selector) = &source.discovery_selector {
            let matcher = dom_query::Matcher::new(selector).map_err(|_| {
                KnowledgeError::Invalid(format!("Invalid discovery selector for {}", source.id))
            })?;
            doc.select_matcher(&matcher)
        } else {
            doc.select("a[href]")
        };
        let links: Vec<String> = link_selection
            .iter()
            .filter_map(|a| a.attr("href"))
            .filter_map(|href| normalize_link(url, &href))
            .filter(|link| {
                source.discovery_selector.is_none()
                    || (link != url.as_str()
                        && Url::parse(link).is_ok_and(|link| source.allows(&link)))
            })
            .collect();
        if source.discovery_selector.is_some()
            && source.seeds.iter().any(|seed| {
                Url::parse(seed).is_ok_and(|seed| {
                    let directory = seed.path().trim_end_matches('/');
                    seed.origin() == url.origin()
                        && (directory == url.path().trim_end_matches('/')
                            || url.path().starts_with(&format!("{directory}/")))
                        && seed.query() == url.query()
                })
            })
            && links.is_empty()
        {
            return Err(KnowledgeError::Invalid(format!(
                "Reviewed documentation directory no longer yields in-scope links: {url}"
            )));
        }
        let directory_page = source.discovery_selector.is_some() && !links.is_empty();
        let mut selection = if let Some(selector) = &source.content_selector {
            let matcher = dom_query::Matcher::new(selector).map_err(|_| {
                KnowledgeError::Invalid(format!("Invalid content selector for {}", source.id))
            })?;
            let selected = doc.select_matcher(&matcher);
            if selected.is_empty() {
                return Err(KnowledgeError::Invalid(format!(
                    "Reviewed content selector no longer matches: {url}"
                )));
            }
            selected
        } else {
            // Several publishers use one article per FAQ section. Selecting the
            // first matching article silently loses the rest of the manual.
            let main = doc.select("main");
            if !main.is_empty() {
                main
            } else {
                doc.select(
                    "article,[itemprop='articleBody'],#mw-content-text,.article-body,.guide_body",
                )
            }
        };
        if selection.is_empty() {
            selection = doc.select("body");
        }
        // Clean only inside the selected article. Publisher layouts sometimes
        // place the entire post feed inside a header; deleting that ancestor
        // would remove the reviewed body itself. Moderation forms also contain
        // real articles, so retain forms and remove their controls instead.
        selection.select("script,style,noscript,nav,header,footer,input,textarea,select,button,aside,[role='navigation'],.mw-editsection,.toc,.cookie-banner").remove();
        // Keep section levels through formatted_text without interpreting any
        // publisher text as HTML. Links were collected before replacing nodes.
        for (tag, marker) in [
            ("h1", "#"),
            ("h2", "##"),
            ("h3", "###"),
            ("h4", "####"),
            ("h5", "#####"),
            ("h6", "######"),
        ] {
            for heading in doc.select(tag).iter() {
                let text = heading.text();
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    heading.set_text(&format!("{marker} {text}"));
                }
            }
        }
        // A reviewed discovery selector identifies directory navigation, not
        // documentation. Traverse it without publishing the listing as a manual.
        let body = if directory_page {
            String::new()
        } else {
            selection.formatted_text().to_string()
        };
        Extracted {
            title: if title.is_empty() {
                title_fallback
            } else {
                title
            },
            body,
            links,
        }
    } else {
        return Err(KnowledgeError::Invalid(format!(
            "Unsupported documentation content type {content_type}: {url}"
        )));
    };
    result.body = normalize_text(&result.body);
    if result.body.len() > MAX_TEXT {
        return Err(KnowledgeError::Invalid(format!(
            "Extracted document exceeds 2 MiB: {url}"
        )));
    }
    if !substantive(source, &result.body)
        && !(source.discover_links
            && result
                .links
                .iter()
                .any(|link| Url::parse(link).is_ok_and(|url| source.allows(&url))))
    {
        return Err(KnowledgeError::Invalid(format!(
            "Documentation has no substantive body: {url}"
        )));
    }
    let beginning = result
        .body
        .chars()
        .take(1500)
        .collect::<String>()
        .to_ascii_lowercase();
    if [
        "verify you are human",
        "enable javascript and cookies to continue",
        "checking your browser",
        "access denied",
        "attention required!",
        "sign in to confirm your age",
    ]
    .iter()
    .any(|marker| beginning.contains(marker))
    {
        return Err(KnowledgeError::Network(format!(
            "Publisher returned an access challenge instead of documentation: {url}"
        )));
    }
    result.title = result.title.chars().take(400).collect();
    result.links.sort();
    result.links.dedup();
    result
        .links
        .retain(|link| Url::parse(link).is_ok_and(|u| source.allows(&u)));
    Ok(result)
}

pub(crate) fn substantive(source: &Source, body: &str) -> bool {
    // A reviewed article-body selector identifies real content even for a
    // one-sentence FAQ. The generic heuristic only distinguishes landing pages.
    !body.trim().is_empty() && (source.content_selector.is_some() || body.chars().count() >= 100)
}

fn normalize_text(text: &str) -> String {
    let mut body = String::new();
    let mut empty = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if !empty && !body.is_empty() {
                body.push('\n');
            }
            empty = true;
        } else {
            body.push_str(line);
            body.push('\n');
            empty = false;
        }
    }
    body.trim().to_owned()
}

fn normalize_link(base: &Url, href: &str) -> Option<String> {
    let mut url = base.join(href).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn markdown_links(base: &Url, text: &str) -> Vec<String> {
    text.split("](")
        .skip(1)
        .filter_map(|part| part.split(')').next())
        .filter_map(|href| normalize_link(base, href.split_whitespace().next().unwrap_or("")))
        .collect()
}

pub(crate) fn sitemap_links(bytes: &[u8]) -> Result<Vec<String>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| KnowledgeError::Invalid("Sitemap is not UTF-8".into()))?;
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(KnowledgeError::Invalid(
            "Sitemap entities are not supported".into(),
        ));
    }
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut in_loc = false;
    let mut location = String::new();
    let mut links = Vec::new();
    loop {
        use quick_xml::events::Event;
        match reader
            .read_event()
            .map_err(|e| KnowledgeError::Invalid(format!("Invalid sitemap: {e}")))?
        {
            Event::Start(element) => {
                if in_loc {
                    return Err(KnowledgeError::Invalid(
                        "Sitemap location contains nested markup".into(),
                    ));
                }
                if element.local_name().as_ref() == "loc" {
                    in_loc = true;
                    location.clear();
                }
            }
            Event::End(element) if element.local_name().as_ref() == "loc" && in_loc => {
                let value = location.trim();
                if value.is_empty() || value.len() > 4096 {
                    return Err(KnowledgeError::Invalid(
                        "Invalid sitemap location length".into(),
                    ));
                }
                links.push(value.to_string());
                if links.len() > 50_000 {
                    return Err(KnowledgeError::Invalid(
                        "Sitemap exceeds 50,000 entries".into(),
                    ));
                }
                in_loc = false;
            }
            Event::Text(text) if in_loc => {
                location.push_str(text.as_ref());
            }
            Event::GeneralRef(reference) if in_loc => {
                let name = reference.as_ref();
                let encoded = format!("&{name};");
                let decoded = quick_xml::escape::unescape(&encoded)
                    .map_err(|e| KnowledgeError::Invalid(e.to_string()))?;
                location.push_str(&decoded);
            }
            Event::CData(text) if in_loc => {
                location.push_str(text.as_ref());
            }
            Event::Eof => {
                if in_loc {
                    return Err(KnowledgeError::Invalid("Unclosed sitemap location".into()));
                }
                break;
            }
            _ => {}
        }
    }
    Ok(links)
}

pub(crate) fn chunks(title: &str, text: &str) -> Vec<Chunk> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut line_offset = 0;
    let mut headings = Vec::new();
    for line in text.split_inclusive('\n') {
        let marker_len = line.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&marker_len)
            && let Some(heading) = line[marker_len..].strip_prefix(' ')
            && !heading.trim().is_empty()
        {
            headings.push((line_offset, heading.trim(), line_offset + line.len()));
        }
        line_offset += line.len();
    }
    let mut sections = Vec::new();
    let mut pending_start = 0;
    if let Some(&(offset, _, _)) = headings.first()
        && text[..offset].chars().count() > CHUNK_CHARS / 2
    {
        sections.push((0, title, 0));
        pending_start = offset;
    }
    for (index, &(_, heading, body_start)) in headings.iter().enumerate() {
        let end = headings.get(index + 1).map_or(text.len(), |entry| entry.0);
        // Short preludes and heading-only ancestors belong to the next real
        // passage. Even a one-character setting is content, never discarded.
        let content = text[body_start..end].trim_start();
        if !content.is_empty() {
            sections.push((pending_start, heading, end - content.len()));
            pending_start = end;
        }
    }
    let mut section = 0;
    while start < text.len() {
        while section < sections.len() && sections[section].0 <= start {
            section += 1;
        }
        let heading = if section == 0 {
            title
        } else {
            sections[section - 1].1
        };
        let content_start = if section == 0 {
            0
        } else {
            sections[section - 1].2
        };
        let section_end = sections.get(section).map_or(text.len(), |entry| entry.0);
        let remaining = &text[start..section_end];
        let hard_end = remaining
            .char_indices()
            .nth(CHUNK_CHARS)
            .map_or(remaining.len(), |(index, _)| index);
        let mut end = start + text_boundary(remaining, hard_end);
        if end <= content_start && content_start < start + hard_end {
            // Do not let a paragraph boundary undo the prelude/ancestor merge
            // when the first content already fits within this chunk's limit.
            end = content_start
                + text_boundary(
                    &text[content_start..section_end],
                    start + hard_end - content_start,
                );
        }
        let body = &text[start..end];
        result.push(Chunk {
            heading: heading.chars().take(200).collect(),
            body: body.to_owned(),
            offset: start,
            vector: Vec::new(),
        });
        if end == text.len() {
            break;
        }
        if end == section_end {
            start = end;
            continue;
        }
        // Context overlap stays within its section and starts at a complete
        // word when possible. No bytes are trimmed from the persisted body.
        let overlap = text[start..end]
            .char_indices()
            .rev()
            .nth(160)
            .map_or(end, |(index, _)| start + index);
        let overlap = text[overlap..end]
            .char_indices()
            .find(|(_, character)| character.is_whitespace())
            .map_or(overlap, |(index, character)| {
                overlap + index + character.len_utf8()
            });
        start = if overlap > start { overlap } else { end };
    }
    result
}

/// Prefer a natural boundary in the latter half of the allowed prefix, keeping
/// both halves nonempty when splitting. The fallback is always a UTF-8 boundary.
pub(crate) fn text_boundary(text: &str, limit: usize) -> usize {
    let mut limit = limit.min(text.len());
    while !text.is_char_boundary(limit) {
        limit -= 1;
    }
    if limit == text.len() {
        return limit;
    }
    let prefix = &text[..limit];
    let boundary = prefix.rfind("\n\n").map(|index| index + 2);
    let boundary = boundary
        .filter(|index| *index > limit / 2)
        .or_else(|| {
            prefix
                .rfind('\n')
                .map(|index| index + 1)
                .filter(|index| *index > limit / 2)
        })
        .or_else(|| {
            prefix
                .char_indices()
                .rev()
                .find(|(index, character)| *index >= limit / 2 && character.is_whitespace())
                .map(|(index, character)| index + character.len_utf8())
        });
    boundary.unwrap_or(limit)
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn document_id(source_id: &str, url: &str) -> String {
    digest(format!("{source_id}\n{url}").as_bytes())[..32].to_owned()
}
