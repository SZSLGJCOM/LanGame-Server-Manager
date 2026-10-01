use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type Reads = BTreeMap<PathBuf, BTreeMap<PathBuf, u64>>;
static READS: OnceLock<Mutex<Reads>> = OnceLock::new();

// Test fixtures own disjoint absolute roots. Normalize spelling without opening
// files, so a moved or removed source remains queryable after the operation.
fn key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().replace('/', "\\");
        let text = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
        };
        PathBuf::from(text.to_lowercase())
    }
    #[cfg(not(windows))]
    path.to_owned()
}

pub(crate) struct ReadProbe {
    root: PathBuf,
}

impl ReadProbe {
    pub(crate) fn bytes(&self, path: &Path) -> u64 {
        let reads = READS.get().unwrap().lock().unwrap();
        reads[&self.root].get(&key(path)).copied().unwrap_or(0)
    }
}

impl Drop for ReadProbe {
    fn drop(&mut self) {
        READS.get().unwrap().lock().unwrap().remove(&self.root);
    }
}

pub(crate) fn register(root: &Path) -> ReadProbe {
    assert!(root.is_absolute());
    let root = key(root);
    let mut reads = READS.get_or_init(Mutex::default).lock().unwrap();
    assert!(
        reads
            .keys()
            .all(|other| !root.starts_with(other) && !other.starts_with(&root)),
        "archive read probes must have disjoint fixture roots"
    );
    reads.insert(root.clone(), BTreeMap::new());
    ReadProbe { root }
}

/// Record actual successful hash reads or a completed copy's input position.
pub(crate) fn record(path: &Path, bytes: u64) {
    let Some(reads) = READS.get() else {
        return;
    };
    let path = key(path);
    let mut reads = reads.lock().unwrap();
    if let Some((_, files)) = reads.iter_mut().find(|(root, _)| path.starts_with(root)) {
        *files.entry(path).or_default() += bytes;
    }
}
