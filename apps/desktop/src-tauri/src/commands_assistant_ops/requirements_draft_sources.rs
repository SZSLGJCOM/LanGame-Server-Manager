const ASSISTANT_DRAFT_BYTES: usize = 16 * 1024;
const ASSISTANT_DRAFT_SOURCES: usize = 128;

#[derive(Debug)]
struct AssistantRequirementSource {
    id: String,
    text: String,
    readable: bool,
}

fn assistant_requirement_sources(request: &str) -> Result<Vec<AssistantRequirementSource>, String> {
    if request.trim().is_empty() || request.len() > ASSISTANT_DRAFT_BYTES {
        return Err(String::from(
            "The complete request must be nonempty and within 16 KiB; it was not truncated.",
        ));
    }
    let safe = redact_assistant_provider_text(request);
    // Preserve the full-request redactor's context (including multiline values).
    // If several regions changed, conservatively hide the interval between them.
    // The file redactor preserves safe JSON formatting only after checking all
    // keys. Value equality alone would overlook credentials in duplicate keys.
    let unchanged_json =
        crate::assistant::redact_assistant_file_text("request", request) == request;
    let hidden = if request == safe || unchanged_json {
        None
    } else {
        let prefix = request
            .bytes()
            .zip(safe.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = request
            .bytes()
            .rev()
            .zip(safe.bytes().rev())
            .take(request.len().min(safe.len()).saturating_sub(prefix))
            .take_while(|(a, b)| a == b)
            .count();
        Some(prefix..request.len() - suffix)
    };
    let quoted_secrets = if hidden.is_some() {
        assistant_draft_recognized_secret_values(request, &safe)
    } else {
        Some(Vec::new())
    };
    let mut sources = Vec::new();
    let mut position = 0;
    for sentence in
        request.split_inclusive([',', ';', '，', '、', '；', '。', '！', '？', '\n', '\r'])
    {
        let sentence_bytes = sentence.len();
        let offset = position + sentence.len() - sentence.trim_start().len();
        let sentence = sentence.trim();
        let mut chunk_start = 0;
        while chunk_start < sentence.len() {
            let mut chunk_end = (chunk_start + 512).min(sentence.len());
            while !sentence.is_char_boundary(chunk_end) {
                chunk_end -= 1;
            }
            let text = &sentence[chunk_start..chunk_end];
            let start = offset + chunk_start;
            let end = offset + chunk_end;
            if !text.trim().is_empty() {
                if sources.len() == ASSISTANT_DRAFT_SOURCES {
                    return Err(String::from(
                        "The request needs more than 128 source fragments; no fragments were silently discarded.",
                    ));
                }
                let readable = !hidden
                    .as_ref()
                    .is_some_and(|range| start < range.end && end > range.start)
                    && quoted_secrets.as_ref().is_some_and(|values| {
                        !values
                            .iter()
                            .any(|value| text.contains(*value) || value.contains(text))
                    })
                    && redact_assistant_provider_text(text) == text;
                sources.push(AssistantRequirementSource {
                    id: format!("request_{}", sources.len() + 1),
                    text: text.into(),
                    readable,
                });
            }
            chunk_start = chunk_end;
        }
        position += sentence_bytes;
    }
    Ok(sources)
}

// Reuse only spans already masked by the common redactor. Exact, unambiguous
// unchanged anchors let us also hide unlabeled repetitions in other clauses.
// If formatting or ambiguous anchors prevent correspondence, no raw clause is
// exposed; this is a clarification gap, never permission to omit a requirement.
fn assistant_draft_recognized_secret_values<'a>(raw: &'a str, safe: &str) -> Option<Vec<&'a str>> {
    let pieces = safe.split("[REDACTED]").collect::<Vec<_>>();
    if pieces.len() < 2 {
        return None;
    }
    let mut remaining = raw.strip_prefix(pieces[0])?;
    let mut values = Vec::new();
    for (index, anchor) in pieces.iter().enumerate().skip(1) {
        let value = if index + 1 == pieces.len() {
            let value = remaining.strip_suffix(*anchor)?;
            remaining = "";
            value
        } else {
            if anchor.is_empty() {
                return None;
            }
            let mut matches = remaining.match_indices(*anchor);
            let (offset, _) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            let value = &remaining[..offset];
            remaining = &remaining[offset + anchor.len()..];
            value
        };
        if !value.is_empty() {
            values.push(value);
        }
    }
    Some(values)
}

fn assistant_draft_source_catalog(sources: &[AssistantRequirementSource]) -> Value {
    json!({"sources":sources.iter().map(|source| json!({
        "id":source.id,"text":if source.readable {source.text.as_str()} else {"[REDACTED]"},
        "readable":source.readable,
    })).collect::<Vec<_>>(),"hasUnreadableSources":sources.iter().any(|source| !source.readable),
    "note":"Source IDs refer to exact contiguous request fragments. Long clauses may span consecutive IDs. Unreadable fragments cannot be automatically interpreted or silently omitted; clarify the request or enter sensitive values in settings. Finishing this draft does not authorize any operation or prove semantic completeness."})
}
