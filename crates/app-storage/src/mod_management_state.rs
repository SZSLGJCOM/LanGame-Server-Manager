//! On-disk identity shared by Mod lifecycle operations and storage migration.
use std::path::Path;

use sha2::{Digest, Sha256};

pub const DIRECTORY_PREFIX: &str = ".langame-mod-state-";

/// The caller resolves and validates the target before deriving its identity.
pub fn target_identity(resolved: &Path) -> String {
    resolved.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// This name allocates a state directory; owner.json is its lasting identity.
/// Migration preserves the name so archived payload indices and journals stay valid.
pub fn initial_directory_name(identity: &str) -> String {
    let digest = Sha256::digest(identity.as_bytes());
    let suffix: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{DIRECTORY_PREFIX}{suffix}")
}

pub fn is_state_directory_name(name: &str) -> bool {
    name.strip_prefix(DIRECTORY_PREFIX).is_some_and(|suffix| {
        suffix.len() == 64
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
