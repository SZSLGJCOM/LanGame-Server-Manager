use std::collections::BTreeMap;

use super::{
    BLOCK_SIZE, MediaCacheError, MediaKind, MediaResponse, OBJECT_LIMIT, full,
    http::{Context, Reply},
    meta::Entry,
    response,
};

#[derive(Clone, Copy)]
pub(super) enum RequestedRange {
    From(u64, Option<u64>),
    Suffix(u64),
}
impl RequestedRange {
    pub fn parse(value: &str) -> Result<Self, MediaCacheError> {
        let value = value
            .trim()
            .strip_prefix("bytes=")
            .ok_or(MediaCacheError::Range)?;
        if value.contains(',') {
            return Err(MediaCacheError::Range);
        }
        let (start, end) = value.split_once('-').ok_or(MediaCacheError::Range)?;
        let number = |value: &str| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MediaCacheError::Range);
            }
            value.parse::<u64>().map_err(|_| MediaCacheError::Range)
        };
        if start.is_empty() {
            let suffix = number(end)?;
            if suffix == 0 {
                return Err(MediaCacheError::Range);
            }
            Ok(Self::Suffix(suffix))
        } else {
            let start = number(start)?;
            let end = if end.is_empty() {
                None
            } else {
                Some(number(end)?)
            };
            if end.is_some_and(|end| end < start) {
                return Err(MediaCacheError::Range);
            }
            Ok(Self::From(start, end))
        }
    }
    fn span(self, total: u64, kind: MediaKind) -> Result<(u64, u64), MediaCacheError> {
        if total == 0 {
            return Err(MediaCacheError::Range);
        }
        let (start, end) = match self {
            Self::From(start, end) => (start, end.unwrap_or(total - 1).min(total - 1)),
            Self::Suffix(length) => (total.saturating_sub(length), total - 1),
        };
        if start >= total {
            return Err(MediaCacheError::Range);
        }
        let end = if kind == MediaKind::Video {
            end.min(start.saturating_add(BLOCK_SIZE - 1))
        } else {
            end
        };
        if end - start + 1 > OBJECT_LIMIT as u64 {
            return Err(MediaCacheError::TooLarge);
        }
        Ok((start, end))
    }
    fn probe(self) -> (u64, u64) {
        match self {
            Self::From(start, _) => {
                let start = start / BLOCK_SIZE * BLOCK_SIZE;
                (start, start.saturating_add(BLOCK_SIZE - 1))
            }
            Self::Suffix(_) => (0, 0),
        }
    }
}

fn content_range(value: Option<&str>) -> Result<(u64, u64, u64), MediaCacheError> {
    let parsed = value
        .and_then(|value| value.strip_prefix("bytes "))
        .and_then(|value| value.split_once('/'))
        .and_then(|(span, total)| Some((span.split_once('-')?, total.parse::<u64>().ok()?)))
        .and_then(|((start, end), total)| {
            Some((start.parse::<u64>().ok()?, end.parse::<u64>().ok()?, total))
        });
    match parsed {
        Some((start, end, total)) if start <= end && end < total => Ok((start, end, total)),
        _ => Err(MediaCacheError::InvalidResponse(
            "invalid Content-Range".into(),
        )),
    }
}
fn unsatisfied(entry: &Entry) -> MediaResponse {
    MediaResponse {
        status: 416,
        headers: vec![
            ("Content-Range".into(), format!("bytes */{}", entry.total)),
            ("Content-Length".into(), "0".into()),
        ],
        body: Vec::new(),
        final_url: entry.source.clone(),
    }
}
fn slice_full(
    mut value: MediaResponse,
    range: RequestedRange,
    kind: MediaKind,
    head: bool,
) -> Result<MediaResponse, MediaCacheError> {
    let total = value.body.len() as u64;
    let (start, end) = match range.span(total, kind) {
        Ok(span) => span,
        Err(MediaCacheError::Range) => {
            value.status = 416;
            value.body.clear();
            value
                .headers
                .retain(|(key, _)| !key.eq_ignore_ascii_case("content-length"));
            value.headers.push(("Content-Length".into(), "0".into()));
            value
                .headers
                .push(("Content-Range".into(), format!("bytes */{total}")));
            return Ok(value);
        }
        Err(error) => return Err(error),
    };
    value.status = 206;
    value.body = if head {
        Vec::new()
    } else {
        value.body[start as usize..=end as usize].to_vec()
    };
    value
        .headers
        .retain(|(key, _)| !key.eq_ignore_ascii_case("content-length"));
    value
        .headers
        .push(("Content-Length".into(), (end - start + 1).to_string()));
    value.headers.push((
        "Content-Range".into(),
        format!("bytes {start}-{end}/{total}"),
    ));
    Ok(value)
}

pub(super) async fn respond(
    context: &Context<'_>,
    sources: &[String],
    mut old: Option<Entry>,
    requested: RequestedRange,
    head: bool,
) -> Result<MediaResponse, MediaCacheError> {
    if old.as_ref().is_some_and(|entry| entry.full.is_some()) {
        return slice_full(
            full::respond(context, sources, old, false).await?,
            requested,
            context.kind,
            head,
        );
    }
    if !head
        && old
            .as_ref()
            .is_some_and(|entry| entry.headers.validator().is_none())
    {
        old = None;
    }
    let mut loaded = BTreeMap::new();
    let mut state = "hit";
    let mut entry = if let Some(entry) = old {
        entry
    } else {
        state = "miss";
        let probe = requested.probe();
        let range = format!("bytes={}-{}", probe.0, probe.1);
        let mut failure = MediaCacheError::Rejected;
        let mut result = None;
        for (index, source) in sources.iter().enumerate() {
            match context
                .fetch(
                    source,
                    if head { None } else { Some(&range) },
                    None,
                    head,
                    false,
                    context.source_deadline(sources.len() - index),
                )
                .await
            {
                Ok(reply) => {
                    result = Some(reply);
                    break;
                }
                Err(error) => {
                    if !error.fallback() {
                        return Err(error);
                    }
                    crate::record_failure(source);
                    failure = error;
                }
            }
        }
        let reply = result.ok_or(failure)?;
        if context.kind == MediaKind::Video && reply.headers.freshness(context.now()).no_store {
            return Err(MediaCacheError::InvalidResponse(
                "native range media prohibits persistent caching".into(),
            ));
        }
        if head && reply.status == 200 {
            let total = reply.length.ok_or_else(|| {
                MediaCacheError::InvalidResponse("HEAD without Content-Length".into())
            })?;
            crate::record_success(&reply.url);
            let entry = Entry::new(reply.url, reply.headers, total, context.now());
            context.store.put(context.key, entry, Vec::new()).await?
        } else if reply.status == 200 {
            crate::record_success(&reply.url);
            let entry = Entry::new(
                reply.url,
                reply.headers,
                reply.body.len() as u64,
                context.now(),
            );
            let entry = context
                .store
                .put(context.key, entry, vec![(None, reply.body.clone())])
                .await?;
            return slice_full(
                response(&entry, reply.body, None, false, "miss"),
                requested,
                context.kind,
                head,
            );
        } else if reply.status == 416 {
            if let Some(total) = reply
                .content_range
                .as_deref()
                .and_then(|value| value.strip_prefix("bytes */"))
                .and_then(|value| value.parse().ok())
            {
                crate::record_success(&reply.url);
                return Ok(unsatisfied(&Entry::new(
                    reply.url,
                    reply.headers,
                    total,
                    context.now(),
                )));
            }
            return Err(MediaCacheError::Range);
        } else {
            if reply.status != 206 {
                return Err(MediaCacheError::InvalidResponse(
                    "range request did not return 206".into(),
                ));
            }
            let (start, end, total) = content_range(reply.content_range.as_deref())?;
            if start != probe.0
                || end != probe.1.min(total - 1)
                || reply.body.len() as u64 != end - start + 1
            {
                return Err(MediaCacheError::InvalidResponse(
                    "incorrect initial range response".into(),
                ));
            }
            if reply.headers.validator().is_none() {
                return Err(MediaCacheError::InvalidResponse(
                    "range media requires a strong ETag".into(),
                ));
            }
            crate::record_success(&reply.url);
            let entry = Entry::new(reply.url, reply.headers, total, context.now());
            let complete_block = start.is_multiple_of(BLOCK_SIZE)
                && end == start.saturating_add(BLOCK_SIZE - 1).min(total - 1);
            if complete_block {
                loaded.insert(start, reply.body.clone());
            }
            context
                .store
                .put(
                    context.key,
                    entry,
                    if complete_block {
                        vec![(Some(start), reply.body)]
                    } else {
                        Vec::new()
                    },
                )
                .await?
        }
    };
    let span = match requested.span(entry.total, context.kind) {
        Ok(span) => span,
        Err(MediaCacheError::Range) => return Ok(unsatisfied(&entry)),
        Err(error) => return Err(error),
    };
    if state == "hit" && !entry.fresh(context.now()) {
        let conditional = entry
            .headers
            .etag
            .as_deref()
            .map(|value| ("If-None-Match", value))
            .or_else(|| {
                entry
                    .headers
                    .last_modified
                    .as_deref()
                    .map(|value| ("If-Modified-Since", value))
            });
        match context
            .fetch(
                &entry.source,
                Some("bytes=0-0"),
                conditional,
                false,
                true,
                context.deadline,
            )
            .await
        {
            Ok(reply) if reply.status == 304 && conditional.is_some() => {
                entry.revalidate(reply.headers, context.now());
            }
            Ok(reply) if reply.status == 206 => {
                if !same_generation(&entry, &reply, (0, 0)) {
                    return invalidate(context, MediaCacheError::GenerationChanged).await;
                }
                entry.revalidate(reply.headers, context.now());
            }
            Ok(_) => return invalidate(context, MediaCacheError::GenerationChanged).await,
            Err(error) if error.offline() && entry.freshness.stale_allowed => {
                crate::record_failure(&entry.source);
                if head {
                    return Ok(response(&entry, Vec::new(), Some(span), true, "stale"));
                }
                if let Some(bytes) = cached_range(context, &entry, span).await? {
                    return Ok(response(&entry, bytes, Some(span), false, "stale"));
                }
                return invalidate(context, error).await;
            }
            Err(error) => {
                if error.fallback() {
                    crate::record_failure(&entry.source);
                }
                return invalidate(context, error).await;
            }
        }
        if context.kind == MediaKind::Video && entry.freshness.no_store {
            return invalidate(
                context,
                MediaCacheError::InvalidResponse(
                    "native range media prohibits persistent caching".into(),
                ),
            )
            .await;
        }
        crate::record_success(&entry.source);
        entry = context.store.put(context.key, entry, Vec::new()).await?;
    }
    if head {
        return Ok(response(&entry, Vec::new(), Some(span), true, state));
    }
    let mut body = Vec::with_capacity((span.1 - span.0 + 1) as usize);
    let mut start = span.0 / BLOCK_SIZE * BLOCK_SIZE;
    while start <= span.1 {
        let mut bytes = loaded.remove(&start);
        if bytes.is_none()
            && let Some(blob) = entry.blocks.get(&start)
        {
            bytes = context.store.read(blob).await?;
        }
        bytes = bytes.filter(|bytes| bytes.len() as u64 == BLOCK_SIZE.min(entry.total - start));
        let bytes = if let Some(bytes) = bytes {
            bytes
        } else {
            state = "miss";
            let end = start.saturating_add(BLOCK_SIZE - 1).min(entry.total - 1);
            let range = format!("bytes={start}-{end}");
            let validator = entry
                .headers
                .validator()
                .ok_or(MediaCacheError::GenerationChanged)?;
            let reply = match context
                .fetch(
                    &entry.source,
                    Some(&range),
                    Some(("If-Range", validator.value())),
                    false,
                    true,
                    context.deadline,
                )
                .await
            {
                Ok(reply) => reply,
                Err(error) => {
                    if error.fallback() {
                        crate::record_failure(&entry.source);
                    }
                    return invalidate(context, error).await;
                }
            };
            if !same_generation(&entry, &reply, (start, end)) {
                return invalidate(context, MediaCacheError::GenerationChanged).await;
            }
            entry.revalidate(reply.headers, context.now());
            if context.kind == MediaKind::Video && entry.freshness.no_store {
                return invalidate(
                    context,
                    MediaCacheError::InvalidResponse(
                        "native range media prohibits persistent caching".into(),
                    ),
                )
                .await;
            }
            crate::record_success(&entry.source);
            let bytes = reply.body;
            entry = context
                .store
                .put(context.key, entry, vec![(Some(start), bytes.clone())])
                .await?;
            bytes
        };
        let from = span.0.saturating_sub(start) as usize;
        let to = (span.1.min(start + bytes.len() as u64 - 1) - start + 1) as usize;
        body.extend_from_slice(
            bytes
                .get(from..to)
                .ok_or(MediaCacheError::GenerationChanged)?,
        );
        if span.1 - start < BLOCK_SIZE {
            break;
        }
        start = start
            .checked_add(BLOCK_SIZE)
            .ok_or(MediaCacheError::Range)?;
    }
    Ok(response(&entry, body, Some(span), false, state))
}

fn same_generation(entry: &Entry, reply: &Reply, expected: (u64, u64)) -> bool {
    reply.status == 206
        && reply.url == entry.source
        && reply.headers.validator() == entry.headers.validator()
        && content_range(reply.content_range.as_deref()).is_ok_and(|(start, end, total)| {
            (start, end) == expected
                && total == entry.total
                && reply.body.len() as u64 == end - start + 1
        })
}
async fn invalidate(
    context: &Context<'_>,
    error: MediaCacheError,
) -> Result<MediaResponse, MediaCacheError> {
    context.store.remove(context.key).await?;
    Err(error)
}
async fn cached_range(
    context: &Context<'_>,
    entry: &Entry,
    span: (u64, u64),
) -> Result<Option<Vec<u8>>, MediaCacheError> {
    let mut result = Vec::new();
    let mut start = span.0 / BLOCK_SIZE * BLOCK_SIZE;
    while start <= span.1 {
        let Some(blob) = entry.blocks.get(&start) else {
            return Ok(None);
        };
        let Some(bytes) = context.store.read(blob).await? else {
            return Ok(None);
        };
        if bytes.len() as u64 != BLOCK_SIZE.min(entry.total - start) {
            return Ok(None);
        }
        let from = span.0.saturating_sub(start) as usize;
        let to = (span.1.min(start + bytes.len() as u64 - 1) - start + 1) as usize;
        let Some(bytes) = bytes.get(from..to) else {
            return Ok(None);
        };
        result.extend_from_slice(bytes);
        if span.1 - start < BLOCK_SIZE {
            break;
        }
        start = start
            .checked_add(BLOCK_SIZE)
            .ok_or(MediaCacheError::Range)?;
    }
    Ok(Some(result))
}
