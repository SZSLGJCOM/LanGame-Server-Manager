use crate::{KnowledgeError, Result, check_cancel, embedding, extract};
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

pub(crate) fn prepare(
    title: &str,
    body: &str,
    encoder: &embedding::Embedder,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<extract::Chunk>> {
    check_deadline(deadline)?;
    let mut chunks = split_to_token_budget(
        title,
        extract::chunks(title, body),
        |text| encoder.token_count(text),
        cancel,
    )?;
    for batch in chunks.chunks_mut(1) {
        check_cancel(cancel)?;
        check_deadline(deadline)?;
        let texts: Vec<String> = batch
            .iter()
            .map(|chunk| embedding_text(title, chunk))
            .collect();
        let vectors = encoder.encode_batch(&texts, cancel)?;
        check_deadline(deadline)?;
        for (chunk, vector) in batch.iter_mut().zip(vectors) {
            chunk.vector = vector;
        }
    }
    Ok(chunks)
}

pub(crate) fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(KnowledgeError::Unavailable(
            "Source exceeded its synchronization deadline; previous snapshot retained".into(),
        ));
    }
    Ok(())
}

fn embedding_text(title: &str, chunk: &extract::Chunk) -> String {
    // Metadata remains complete in the document/citation. Bounded context labels
    // leave room for the complete body segment within the model's token window.
    format!(
        "{}\n{}\n{}",
        title.chars().take(96).collect::<String>(),
        chunk.heading.chars().take(96).collect::<String>(),
        chunk.body
    )
}

fn split_to_token_budget(
    title: &str,
    chunks: Vec<extract::Chunk>,
    count: impl Fn(&str) -> Result<usize>,
    cancel: &AtomicBool,
) -> Result<Vec<extract::Chunk>> {
    let mut pending = VecDeque::from(chunks);
    let mut output = Vec::new();
    while let Some(mut chunk) = pending.pop_front() {
        check_cancel(cancel)?;
        if output.len() + pending.len() >= 4096 {
            return Err(KnowledgeError::Unavailable(
                "Document exceeds the 4,096-chunk indexing budget".into(),
            ));
        }
        if count(&embedding_text(title, &chunk))? <= embedding::MAX_TOKENS {
            output.push(chunk);
            continue;
        }
        let boundary = extract::text_boundary(&chunk.body, chunk.body.len() / 2);
        if boundary == 0 {
            return Err(KnowledgeError::Model(
                "Document metadata leaves no room in the embedding token window".into(),
            ));
        }
        let remainder = chunk.body.split_off(boundary);
        pending.push_front(extract::Chunk {
            heading: chunk.heading.clone(),
            body: remainder,
            offset: chunk.offset + boundary,
            vector: Vec::new(),
        });
        pending.push_front(chunk);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_indexing_budget_is_not_reported_as_success() {
        assert!(check_deadline(Instant::now()).is_err());
        assert!(check_deadline(Instant::now() + std::time::Duration::from_secs(60)).is_ok());
    }
    #[test]
    fn overlong_multilingual_passages_are_split_without_losing_any_source_bytes() {
        let body = "服务器配置🔒\n".repeat(900);
        let chunks = vec![extract::Chunk {
            heading: "设置".into(),
            body: body.clone(),
            offset: 0,
            vector: vec![],
        }];
        // Substitute only tokenizer counts; the splitting and byte cursors are real.
        let segments = split_to_token_budget(
            "标题",
            chunks,
            |text| Ok(text.chars().count()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(segments.len() > 3);
        let mut recovered = String::new();
        for segment in segments {
            assert_eq!(segment.offset, recovered.len());
            assert!(embedding_text("标题", &segment).chars().count() <= embedding::MAX_TOKENS);
            recovered.push_str(&segment.body);
        }
        assert_eq!(recovered, body);
    }

    #[test]
    fn token_budget_splits_preserve_complete_words_lines_and_exact_offsets() {
        for body in [
            "Keep the server configuration and world saves together 🔒. ".repeat(90),
            "server_password=secret maximum_players=12 保存配置\n".repeat(90),
        ] {
            let segments = split_to_token_budget(
                "Guide",
                vec![extract::Chunk {
                    heading: "Settings".into(),
                    body: body.clone(),
                    offset: 37,
                    vector: vec![],
                }],
                |text| Ok(text.chars().count()),
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(segments.len() > 3);
            let mut recovered = String::new();
            for segment in segments {
                assert_eq!(segment.offset, 37 + recovered.len());
                assert!(embedding_text("Guide", &segment).chars().count() <= embedding::MAX_TOKENS);
                recovered.push_str(&segment.body);
                if recovered.len() < body.len() {
                    assert!(segment.body.chars().next_back().unwrap().is_whitespace());
                    if body.contains('\n') {
                        assert!(segment.body.ends_with('\n'));
                    }
                }
            }
            assert_eq!(recovered, body);
        }
    }

    #[test]
    fn unbroken_unicode_text_still_obeys_token_budget_without_dropping_characters() {
        let body = "保存🔒".repeat(900);
        let segments = split_to_token_budget(
            "Guide",
            vec![extract::Chunk {
                heading: "Settings".into(),
                body: body.clone(),
                offset: 0,
                vector: vec![],
            }],
            |text| Ok(text.chars().count()),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut recovered = String::new();
        for segment in segments {
            assert_eq!(segment.offset, recovered.len());
            assert!(embedding_text("Guide", &segment).chars().count() <= embedding::MAX_TOKENS);
            recovered.push_str(&segment.body);
        }
        assert_eq!(recovered, body);
    }
}
