use std::path::Path;

use super::managed_config_merge::{
    ManagedConfigMergePlan, ManagedConfigMutation, materialization_error, read_optional_bytes,
    read_required_rendered_file,
};
use crate::StorageError;

fn seed_line(contents: &str) -> Option<&str> {
    contents.lines().rfind(|line| {
        line.split_once('=')
            .is_some_and(|(key, _)| key.trim() == "Seed")
    })
}

pub(super) fn copy_preserving_native_seed(
    source: &Path,
    destination: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let mut rendered = read_required_rendered_file(source, "projectzomboid")?;
    let original = read_optional_bytes(destination)?;
    // The game generates and saves Seed on first launch. A blank guided field
    // leaves it unmanaged; replacing the INI must not regenerate it next time.
    // Only this native-generated value is retained, not retired server options.
    if seed_line(&rendered).is_none()
        && let Some(existing) = original.as_deref()
    {
        let existing = std::str::from_utf8(existing).map_err(|error| {
            materialization_error(
                "projectzomboid",
                destination,
                format!("cannot preserve Seed from non-UTF-8 server INI: {error}"),
            )
        })?;
        if let Some(seed) = seed_line(existing) {
            let newline = if rendered.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            if !rendered.is_empty() && !rendered.ends_with('\n') {
                rendered.push_str(newline);
            }
            rendered.push_str(seed);
            rendered.push_str(newline);
        }
    }
    // Bind preservation to the bytes we read. Concurrent native changes must
    // conflict rather than be overwritten with an older seed.
    files.apply(vec![ManagedConfigMergePlan {
        destination_path: destination.to_owned(),
        replacement: rendered.into_bytes(),
        original,
    }])
}
