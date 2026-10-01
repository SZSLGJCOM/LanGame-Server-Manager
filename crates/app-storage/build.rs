use std::fs;
use std::path::Path;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn main() {
    let migrations_root = Path::new("../../migrations");
    println!("cargo:rerun-if-changed={}", migrations_root.display());

    let mut migration_paths = fs::read_dir(migrations_root)
        .expect("read SQLx migrations")
        .map(|entry| entry.expect("read SQLx migration entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect::<Vec<_>>();
    migration_paths.sort();

    let mut fingerprint = FNV_OFFSET_BASIS;
    for path in migration_paths {
        for byte in path
            .file_name()
            .expect("migration file name")
            .as_encoded_bytes()
            .iter()
            .copied()
            .chain(fs::read(&path).expect("read SQLx migration"))
        {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
        }
    }

    println!("cargo:rustc-env=LANGAME_MIGRATIONS_FINGERPRINT={fingerprint:016x}");
}
