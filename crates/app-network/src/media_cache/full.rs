use super::{
    MediaCacheError, MediaResponse,
    http::Context,
    meta::{Entry, Freshness},
    response,
};

pub(super) async fn respond(
    context: &Context<'_>,
    sources: &[String],
    mut old: Option<Entry>,
    head: bool,
) -> Result<MediaResponse, MediaCacheError> {
    let mut cached = None;
    if let Some(entry) = &old {
        if let Some(blob) = &entry.full
            && context
                .image_sha256
                .is_none_or(|expected| blob.sha256 == expected)
        {
            cached = context.store.read(blob).await?;
        }
        // A disk object's self-consistent metadata is not the authority for a
        // frozen resource. Metadata-only HEAD entries cannot prove its bytes.
        if (entry.full.is_some() || context.image_sha256.is_some()) && cached.is_none() {
            context.store.remove(context.key).await?;
            old = None;
        }
    }
    if let Some(entry) = &old
        && entry.fresh(context.now())
        && (head || cached.is_some())
    {
        return Ok(response(
            entry,
            cached.unwrap_or_default(),
            None,
            head,
            "hit",
        ));
    }
    let mut failure = MediaCacheError::Rejected;
    for (index, url) in sources.iter().enumerate() {
        let conditional = old
            .as_ref()
            .filter(|entry| entry.source == *url && (head || cached.is_some()))
            .and_then(|entry| {
                entry
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
                    })
            });
        let fetched = context
            .fetch(
                url,
                None,
                conditional,
                head,
                false,
                context.source_deadline(sources.len() - index),
            )
            .await;
        match fetched {
            Ok(reply) if reply.status == 304 && conditional.is_some() => {
                let Some(mut entry) = old.clone() else {
                    return Err(MediaCacheError::InvalidResponse(
                        "304 without a cached object".into(),
                    ));
                };
                entry.revalidate(reply.headers, context.now());
                let entry = context.store.put(context.key, entry, Vec::new()).await?;
                crate::record_success(&reply.url);
                return Ok(response(
                    &entry,
                    cached.unwrap_or_default(),
                    None,
                    head,
                    "hit",
                ));
            }
            Ok(reply) if reply.status == 200 => {
                let total = if head {
                    reply.length.ok_or_else(|| {
                        MediaCacheError::InvalidResponse("HEAD response has no length".into())
                    })?
                } else {
                    reply.body.len() as u64
                };
                let mut entry = Entry::new(reply.url.clone(), reply.headers, total, context.now());
                if context.cache_immutable {
                    // Explicitly downloaded assets use their reviewed digest as
                    // their version. HTTP API no-store headers still describe the
                    // response, but do not govern this opt-in bounded asset store.
                    entry.freshness = Freshness {
                        expires_at: u64::MAX,
                        no_store: false,
                        stale_allowed: true,
                    };
                }
                let entry = context
                    .store
                    .put(
                        context.key,
                        entry,
                        if head && context.image_sha256.is_none() {
                            Vec::new()
                        } else {
                            vec![(None, reply.body.clone())]
                        },
                    )
                    .await?;
                crate::record_success(&reply.url);
                return Ok(response(&entry, reply.body, None, head, "miss"));
            }
            Ok(reply) => {
                failure = MediaCacheError::InvalidResponse(format!(
                    "unexpected HTTP {} for complete media",
                    reply.status
                ));
            }
            Err(error) => {
                if !error.fallback() {
                    return Err(error);
                }
                failure = error;
            }
        }
        crate::record_failure(url);
    }
    if failure.offline()
        && let Some(entry) = old.filter(|entry| entry.freshness.stale_allowed)
        && (head || cached.is_some())
    {
        return Ok(response(
            &entry,
            cached.unwrap_or_default(),
            None,
            head,
            "stale",
        ));
    }
    Err(failure)
}
