use std::collections::HashMap;

/// Spread a ranked passage list across documents before pagination. Later
/// rounds retain additional passages from long manuals with their original
/// relative rank, so callers can still page through every candidate.
pub(crate) fn diversify_documents(candidates: &mut [(usize, f32, f32)], document_ids: &[String]) {
    let mut occurrences = HashMap::with_capacity(candidates.len());
    let mut decorated: Vec<_> = candidates
        .iter()
        .copied()
        .enumerate()
        .map(|(rank, candidate)| {
            let occurrence = occurrences
                .entry(document_ids[candidate.0].as_str())
                .or_insert(0usize);
            let key = (*occurrence, rank);
            *occurrence += 1;
            (key, candidate)
        })
        .collect();
    decorated.sort_unstable_by_key(|(key, _)| *key);
    for (slot, (_, candidate)) in candidates.iter_mut().zip(decorated) {
        *slot = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_api_schema_cannot_hide_a_configuration_manual_on_the_first_page() {
        let documents = [
            "api",
            "api",
            "api",
            "configuration",
            "configuration",
            "ports",
            "backups",
        ]
        .map(str::to_owned);
        let mut candidates: Vec<_> = (0..documents.len())
            .map(|index| (index, 0.9 - index as f32 * 0.01, 1.0))
            .collect();
        diversify_documents(&mut candidates, &documents);
        let indices: Vec<_> = candidates.iter().map(|entry| entry.0).collect();
        assert_eq!(indices, [0, 3, 5, 6, 1, 4, 2]);
        assert_eq!(candidates[1].1, 0.9 - 3.0 * 0.01);
        // Pagination slices a single stable order; no candidate is discarded
        // and no continuation re-emits a passage from the preceding page.
        let paged: Vec<_> = candidates
            .chunks(5)
            .flatten()
            .map(|entry| entry.0)
            .collect();
        assert_eq!(paged, indices);
    }

    #[test]
    fn separate_backup_sections_in_one_long_manual_remain_available() {
        let documents = ["manual", "manual", "manual"].map(str::to_owned);
        let mut candidates = [(1, 0.95, 0.2), (0, 0.85, 0.1), (2, 0.8, 0.05)];
        let original = candidates;
        diversify_documents(&mut candidates, &documents);
        assert_eq!(candidates, original);
        diversify_documents(&mut [], &[]);
    }
}
