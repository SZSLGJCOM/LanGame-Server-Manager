use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn safe_relative_path(value: &str, fixture_path: &Path) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let looks_like_windows_drive = value
        .as_bytes()
        .get(1)
        .is_some_and(|separator| *separator == b':');
    let unsafe_component = path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    });
    let unsafe_windows_segment = value.split('/').any(|segment| {
        let normalized = segment.to_ascii_lowercase();
        let stem = normalized.split('.').next().unwrap_or(&normalized);
        let reserved = matches!(stem, "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
            || stem.strip_prefix("com").is_some_and(|index| {
                matches!(index, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
            || stem.strip_prefix("lpt").is_some_and(|index| {
                matches!(index, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
        segment.is_empty()
            || segment
                .chars()
                .any(|character| character.is_control() || "<>:\"|?*".contains(character))
            || segment.ends_with('.')
            || segment.ends_with(' ')
            || reserved
    });

    if value.trim().is_empty()
        || value.contains('\\')
        || looks_like_windows_drive
        || path.is_absolute()
        || unsafe_component
        || unsafe_windows_segment
    {
        return Err(format!(
            "unsafe relative path {value:?} at {}",
            fixture_path.display()
        ));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn unique_system_temp_root(scope: &str) -> PathBuf {
    let safe_scope = scope
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();

    for _ in 0..32 {
        let sequence = TEMP_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = std::env::temp_dir().join(format!("ca-{safe_scope}-{sequence}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => panic!(
                "failed to create acceptance temp root {}: {source}",
                candidate.display()
            ),
        }
    }
    panic!("failed to allocate a unique acceptance temp root")
}

pub(crate) fn resolve_expected_path_tokens(
    argument: &str,
    instance_root: &Path,
    config_dir: &Path,
    install_root: &Path,
    saves_dir: &Path,
) -> String {
    let data_dir = instance_root.join("data");
    let logs_dir = instance_root.join("logs");
    [
        ("{{paths.instance_root}}", instance_root),
        ("{{paths.config_dir}}", config_dir),
        ("{{paths.install_root}}", install_root),
        ("{{paths.saves_dir}}", saves_dir),
        ("{{paths.data_dir}}", data_dir.as_path()),
        ("{{paths.logs_dir}}", logs_dir.as_path()),
    ]
    .into_iter()
    .fold(String::from(argument), |resolved, (token, value)| {
        resolved.replace(token, &value.to_string_lossy())
    })
}

pub(crate) fn expected_launch_argument_matches(
    raw_expected: &str,
    resolved_expected: &str,
    actual: &str,
) -> bool {
    const PATH_TOKENS: [&str; 6] = [
        "{{paths.instance_root}}",
        "{{paths.config_dir}}",
        "{{paths.install_root}}",
        "{{paths.saves_dir}}",
        "{{paths.data_dir}}",
        "{{paths.logs_dir}}",
    ];

    if !PATH_TOKENS
        .into_iter()
        .any(|token| raw_expected.contains(token))
    {
        return resolved_expected == actual;
    }

    resolved_expected
        .chars()
        .map(normalize_path_separator)
        .eq(actual.chars().map(normalize_path_separator))
}

fn normalize_path_separator(character: char) -> char {
    match character {
        '\\' => '/',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_path_tokens_preserve_the_raw_suffix() {
        let instance_root = Path::new("acceptance").join("instance");
        let argument = "-log={{paths.logs_dir}}/server.log";

        assert_eq!(
            resolve_expected_path_tokens(
                argument,
                &instance_root,
                &instance_root.join("config"),
                &instance_root.join("install"),
                &instance_root.join("saves"),
            ),
            format!("-log={}/server.log", instance_root.join("logs").display())
        );
    }

    #[test]
    fn path_token_arguments_compare_mixed_and_native_separators_semantically() {
        let instance_root = Path::new("acceptance").join("instance");
        let raw_expected = "{{paths.logs_dir}}/CoreKeeperServer.log";
        let resolved_expected = resolve_expected_path_tokens(
            raw_expected,
            &instance_root,
            &instance_root.join("config"),
            &instance_root.join("install"),
            &instance_root.join("saves"),
        );
        let native_actual = instance_root
            .join("logs")
            .join("CoreKeeperServer.log")
            .to_string_lossy()
            .into_owned();

        assert!(expected_launch_argument_matches(
            raw_expected,
            &resolved_expected,
            &native_actual,
        ));
        let alternate_separators = native_actual
            .chars()
            .map(|character| match character {
                '/' => '\\',
                '\\' => '/',
                other => other,
            })
            .collect::<String>();
        assert!(expected_launch_argument_matches(
            raw_expected,
            &resolved_expected,
            &alternate_separators,
        ));
    }

    #[test]
    fn literal_arguments_do_not_treat_slashes_as_path_separators() {
        for (expected, actual) in [
            (
                "https://example.test/server",
                r"https:\\example.test\server",
            ),
            ("password/segment", r"password\segment"),
            ("{{paths.unknown}}/value", r"{{paths.unknown}}\value"),
        ] {
            assert!(!expected_launch_argument_matches(
                expected, expected, actual,
            ));
        }
    }

    #[test]
    fn relative_fixture_paths_reject_windows_aliases_and_alternate_streams() {
        let fixture = Path::new("controlled-fixture.json");
        for path in [
            "server.ini:secret",
            "bad<name.ini",
            "CON",
            "nested/aux.txt",
            "nested//server.ini",
            "trailing./server.ini",
            "trailing /server.ini",
        ] {
            assert!(
                safe_relative_path(path, fixture).is_err(),
                "{path} must be rejected"
            );
        }
        assert_eq!(
            safe_relative_path("nested/server.ini", fixture).unwrap(),
            PathBuf::from("nested/server.ini")
        );
    }
}
