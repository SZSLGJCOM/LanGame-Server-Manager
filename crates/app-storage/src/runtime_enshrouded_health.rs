use std::io;
use std::path::Path;

/// Keep Enshrouded's single latest transition and existing checkpoint contract.
pub(super) fn observe(path: &Path, run_id: i64) -> io::Result<Option<String>> {
    super::console_health_evidence::observe(
        super::console_health_evidence::SessionKind::Enshrouded,
        path,
        run_id,
    )
    .map(|lines| lines.into_iter().next())
}
