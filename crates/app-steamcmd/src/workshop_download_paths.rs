//! Retain native success locations independently of the bounded diagnostic tail.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_ITEMS: usize = 8192;

#[derive(Debug, Default)]
pub(crate) struct DownloadPaths(BTreeMap<u64, Option<PathBuf>>);

impl DownloadPaths {
    pub(crate) fn observe_line(&mut self, line: &str) {
        let Some((item, path)) = downloaded_item(line) else {
            return;
        };
        if self.0.len() < MAX_ITEMS || self.0.contains_key(&item) {
            let retained = self.0.entry(item).or_default();
            if path.is_some() {
                *retained = path;
            }
        }
    }

    pub(crate) fn item_path(&self, item: &str) -> Option<&Path> {
        self.0.get(&item.parse::<u64>().ok()?)?.as_deref()
    }

    pub(crate) fn item_succeeded(&self, item: &str) -> bool {
        item.parse::<u64>()
            .ok()
            .is_some_and(|item| self.0.contains_key(&item))
    }
}

fn downloaded_item(line: &str) -> Option<(u64, Option<PathBuf>)> {
    let line = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(stamp, _)| {
            stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
        })
        .map_or(line, |(_, message)| message)
        .trim();
    let (item, destination) = line
        .strip_prefix("Success. Downloaded item ")?
        .split_once(" to ")?;
    let item = item.parse::<u64>().ok().filter(|item| *item > 0)?;
    let destination = destination.trim();
    let path = if let Some(quoted) = destination.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        destination
            .rsplit_once(" (")
            .filter(|(_, suffix)| suffix.ends_with(" bytes)"))
            .map_or(destination, |(path, _)| path)
    };
    if path.is_empty() {
        return None;
    }
    let path = PathBuf::from(path);
    Some((item, path.is_absolute().then_some(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_paths_survive_later_output_and_are_scoped_to_the_item() {
        let path = std::env::temp_dir().join("workshop path/content/42/123456");
        let mut evidence = DownloadPaths::default();
        evidence.observe_line(&format!(
            "[2026-10-17 20:42:45] Success. Downloaded item 123456 to \"{}\" (6 bytes)",
            path.display()
        ));
        for _ in 0..100 {
            evidence.observe_line("later unrelated output");
        }
        assert_eq!(evidence.item_path("123456"), Some(path.as_path()));
        assert!(evidence.item_succeeded("123456"));
        assert!(evidence.item_path("654321").is_none());
        assert!(!evidence.item_succeeded("654321"));
    }

    #[test]
    fn incomplete_or_relative_success_locations_are_not_download_evidence() {
        let mut evidence = DownloadPaths::default();
        for line in [
            "Success. Downloaded item 123456 to cache",
            "Success. Downloaded item 123456 to \"unterminated",
            "ERROR! Download item 123456 failed (Failure).",
            "Success. Downloaded item abc to cache",
        ] {
            evidence.observe_line(line);
        }
        assert!(evidence.item_path("123456").is_none());
        assert!(evidence.item_succeeded("123456"));
    }

    #[test]
    fn runtime_and_malformed_success_lines_do_not_confirm_an_item() {
        let mut evidence = DownloadPaths::default();
        for line in [
            "Steam Console Client (c) Valve Corporation",
            "Loading Steam API...OK",
            "Success. Downloaded item 123456 to \"unterminated",
            "Success. Downloaded item 123456 to \"\"",
            "Success. Downloaded item 123456 to ",
        ] {
            evidence.observe_line(line);
        }
        assert!(!evidence.item_succeeded("123456"));
    }
}
