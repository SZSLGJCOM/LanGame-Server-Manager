use super::*;

const FIXTURE: &str = r#"
"win64"
{
    "version" "1788292693"
    "ostype" "win10"
    "steamcmd_public_all"
    {
        "file" "steamcmd_public_all.zip.9acb456879ee932518117972e2b09b938f19063b"
        "size" "60352"
        "sha2" "6fad0bff904dac6cce1c56d9bdf60915e8041e9e3d12b3ca3baeca31d1e00acc"
    }
    "steamcmd_win64"
    {
        "file" "steamcmd_win64.zip.463af880ca68070eef4c4e7421ec34c4f60ad4bd"
        "size" "2503604"
        "sha2" "744f46f3cf27bbd2d58e07192f5effb161e8bd0ddf41af7c26c070e3dda80be7"
        "zipvz" "steamcmd_win64.zip.vz.e829dda457d38b4a758a9991555fbf60cc976c6d_2030453"
        "sha2vz" "dab8bc836083981f44066405135ba24ce4658cd564919e4228756e44bdb7dd42"
        "IsBootstrapperPackage" "1"
    }
}
"kvsign2" { "win64" "signature" }
"kvsignatures" { "win64" "signature" }
"#;

fn single_package(fields: &str) -> String {
    format!(r#""win64" {{ "version" "123" "package" {{ {fields} }} }}"#)
}

fn package_fields(file: &str, size: u64) -> String {
    format!(
        r#""file" "{file}" "size" "{size}" "sha2" "{}""#,
        "a".repeat(64)
    )
}

#[test]
fn official_manifest_selects_vz_and_preserves_bootstrap_zip_metadata() {
    let manifest = parse_manifest(FIXTURE, "win64").unwrap();
    assert_eq!(manifest.version, "1788292693");
    assert_eq!(manifest.packages.len(), 2);
    let public = &manifest.packages[0];
    assert_eq!(public.name, "steamcmd_public_all");
    assert_eq!(public.file, public.archive_file);
    assert_eq!(public.size, 60352);
    assert!(!public.bootstrapper);
    let bootstrap = &manifest.packages[1];
    assert!(bootstrap.bootstrapper);
    assert!(bootstrap.file.contains(".zip.vz."));
    assert_eq!(bootstrap.size, 2030453);
    assert_eq!(bootstrap.archive_size, 2503604);
    assert_ne!(bootstrap.sha256, bootstrap.archive_sha256);
    assert_eq!(bootstrap.sha256.len(), 64);
}

#[test]
fn platform_selection_supports_win32_and_rejects_missing_or_unsupported_platforms() {
    let win32 = FIXTURE.replace("win64", "win32");
    assert!(parse_manifest(&win32, "win32").is_ok());
    assert!(parse_manifest(&win32, "win64").is_err());
    assert!(parse_manifest(FIXTURE, "linux64").is_err());
}

#[test]
fn supported_keyvalues_syntax_includes_comments_unquoted_tokens_and_crlf() {
    let text = single_package(&package_fields("package.zip.hash", 12));
    let text = text.replace("\"version\" \"123\"", "// comment\r\nversion 123");
    assert!(parse_manifest(&text, "win64").is_ok());
    assert!(parse_manifest(&format!("{text}\r\n// end"), "win64").is_ok());
}

#[test]
fn duplicate_fields_are_rejected_case_insensitively_even_in_ignored_signature_blocks() {
    let fields = package_fields("package.zip.hash", 12);
    let cases = [
        single_package(&format!(r#"{fields} "FILE" "other.zip.hash""#)),
        FIXTURE.replace("\"version\"", "\"version\" \"1\" \"Version\""),
        format!(r#"{FIXTURE} "KVSIGN2" {{ "win64" "duplicate" }}"#),
        FIXTURE.replace("\"signature\"", "\"signature\" \"WIN64\" \"duplicate\""),
    ];
    for text in cases {
        assert!(parse_manifest(&text, "win64").is_err());
    }
}

#[test]
fn invalid_or_unfinished_syntax_is_not_ignored() {
    for suffix in ["}", "{", "dangling", "\"unterminated", "key }"] {
        assert!(parse_manifest(&format!("{FIXTURE}{suffix}"), "win64").is_err());
    }
    for text in [
        FIXTURE
            .trim_end_matches(['\n', ' '])
            .trim_end_matches('}')
            .to_owned(),
        FIXTURE.replace("signature", "bad\\escape"),
        FIXTURE.replace("signature", "contains\u{0}control"),
        FIXTURE.replace("signature", "中文"),
    ] {
        assert!(parse_manifest(&text, "win64").is_err());
    }
}

#[test]
fn package_filenames_cannot_escape_or_alias_the_cache_directory() {
    for file in [
        "../escape.zip",
        "..\\escape.zip",
        "C:evil.zip",
        "//host/file.zip",
        "part%2fother.zip",
        ".",
        "..",
        "name..zip",
        "trailing.",
        "name zip",
        "name\"zip",
        "name?zip",
        "name#zip",
        "name\u{0}zip",
        "路径.zip",
        "",
        "CON",
        "nul.zip",
        "Com1.zip",
        "LPT9.zip",
    ] {
        let text = single_package(&package_fields(file, 12));
        assert!(parse_manifest(&text, "win64").is_err(), "accepted {file:?}");
    }
    let text = single_package(&package_fields(&"a".repeat(256), 12));
    assert!(parse_manifest(&text, "win64").is_err());
}

#[test]
fn integrity_metadata_is_mandatory_and_strict() {
    let fields = package_fields("package.zip.hash", 12);
    for digest in ["", "a", &"g".repeat(64), &"a".repeat(65)] {
        let text = single_package(&fields.replace(&"a".repeat(64), digest));
        assert!(parse_manifest(&text, "win64").is_err());
    }
    for size in ["0", "-1", "+1", "1.5", "18446744073709551616", "536870913"] {
        let text = single_package(&fields.replace("\"12\"", &format!("\"{size}\"")));
        assert!(parse_manifest(&text, "win64").is_err());
    }
    assert!(parse_manifest(&single_package(r#""file" "package.zip""#), "win64").is_err());
    for version in ["", "0", "abc", "-1", "+1", "1.2", "18446744073709551616"] {
        assert!(parse_manifest(&FIXTURE.replace("1788292693", version), "win64").is_err());
    }
}

#[test]
fn incomplete_or_invalid_vz_metadata_is_rejected() {
    let fields = package_fields("package.zip.hash", 12);
    let digest = "b".repeat(64);
    for extra in [
        r#""zipvz" "package.zip.vz.hash_12""#.to_owned(),
        format!(r#""sha2vz" "{digest}""#),
        format!(r#""zipvz" "../package_12" "sha2vz" "{digest}""#),
        format!(r#""zipvz" "package.zip.vz.hash" "sha2vz" "{digest}""#),
        format!(r#""zipvz" "package.zip.vz.hash_0" "sha2vz" "{digest}""#),
        format!(r#""zipvz" "package.zip.vz.hash_536870913" "sha2vz" "{digest}""#),
        r#""zipvz" "package.zip.vz.hash_12" "sha2vz" "bad""#.to_owned(),
        r#""IsBootstrapperPackage" "yes""#.to_owned(),
    ] {
        assert!(parse_manifest(&single_package(&format!("{fields} {extra}")), "win64").is_err());
    }
}

#[test]
fn parser_and_package_set_have_explicit_resource_limits() {
    assert!(parse_manifest(&" ".repeat(MAX_MANIFEST_BYTES + 1), "win64").is_err());
    assert!(parse_manifest(&format!("{FIXTURE} {}", "x {".repeat(6)), "win64").is_err());
    let nested = format!("{FIXTURE} {} leaf value {}", "x {".repeat(5), "}".repeat(5));
    assert!(parse_manifest(&nested, "win64").is_err());
    let long_string = FIXTURE.replace("signature", &"x".repeat(MAX_STRING_BYTES + 1));
    assert!(parse_manifest(&long_string, "win64").is_err());
    let many_tokens = format!(
        "{FIXTURE} ignored {{ {} }}",
        (0..MAX_TOKENS)
            .map(|index| format!("k{index} v "))
            .collect::<String>()
    );
    assert!(many_tokens.len() < MAX_MANIFEST_BYTES);
    assert!(parse_manifest(&many_tokens, "win64").is_err());
    for (count, size) in [(0, 12), (33, 12), (2, MAX_PACKAGE_BYTES)] {
        let packages = (0..count)
            .map(|index| {
                format!(
                    "p{index} {{ {} }}",
                    package_fields(&format!("p{index}.zip"), size)
                )
            })
            .collect::<String>();
        let text = format!("win64 {{ version 123 {packages} }}");
        assert!(parse_manifest(&text, "win64").is_err());
    }
}

#[test]
fn cache_filenames_are_unique_across_packages_and_archive_variants() {
    let fields = package_fields("package.zip.hash", 12);
    let text = format!("win64 {{ version 123 first {{ {fields} }} second {{ {fields} }} }}");
    assert!(parse_manifest(&text, "win64").is_err());
    let digest = "b".repeat(64);
    let first = format!(r#"{fields} "zipvz" "package.zip.vz.hash_10" "sha2vz" "{digest}""#);
    let second = package_fields("PACKAGE.ZIP.VZ.HASH_10", 10);
    let text = format!("win64 {{ version 123 first {{ {first} }} second {{ {second} }} }}");
    assert!(parse_manifest(&text, "win64").is_err());
}
