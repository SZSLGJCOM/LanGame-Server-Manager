/// Computes Enshrouded's native account hash for a Steam (platform 3) value.
///
/// Verified against the native pure function at RVA `0x9f58a0` in executable
/// SHA-256 `4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba`;
/// see `modules/enshrouded/reference-fixtures/2026-10-16-native_account_hash.json`.
/// This does not validate an account, associate a player name with an account,
/// handle other platforms, or establish compatibility with another executable.
pub fn enshrouded_steam_account_hash(steam_id: u64) -> u64 {
    steam_id
        .to_le_bytes()
        .into_iter()
        .fold(0x3f63_b56d_b289_0a16, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::enshrouded_steam_account_hash;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct NativeFixture {
        server_executable_sha256: String,
        hash_function_rva: String,
        steam_platform: u8,
        cases: Vec<NativeCase>,
    }

    #[derive(Deserialize)]
    struct NativeCase {
        platform: u8,
        synthetic_platform_id: String,
        native_hash: String,
    }

    #[test]
    fn steam_hash_matches_independent_native_profile_vectors() {
        let fixture: NativeFixture = serde_json::from_str(include_str!(
            "../../../modules/enshrouded/reference-fixtures/2026-10-16-native_account_hash.json"
        ))
        .expect("native hash evidence must deserialize without numeric coercion");
        assert_eq!(
            fixture.server_executable_sha256,
            "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba"
        );
        assert_eq!(fixture.hash_function_rva, "0x9f58a0");
        assert_eq!(fixture.steam_platform, 3);

        let steam_cases: Vec<_> = fixture
            .cases
            .iter()
            .filter(|case| case.platform == fixture.steam_platform)
            .collect();
        assert_eq!(steam_cases.len(), 8);
        // The native platform-0 cases exercise a separate dispatch path. They
        // are not expectations for this Steam-only function, even for ID 0.
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.platform == 0)
                .count(),
            2
        );

        let mut checked_byte_boundary = false;
        let mut checked_wrapping_boundary = false;
        for case in steam_cases {
            let steam_id = case
                .synthetic_platform_id
                .parse::<u64>()
                .expect("synthetic Steam value must be an exact u64");
            let expected = case
                .native_hash
                .parse::<u64>()
                .expect("native output must be an exact u64");
            assert_eq!(
                enshrouded_steam_account_hash(steam_id),
                expected,
                "independent platform-3 native vector for {steam_id}"
            );
            checked_byte_boundary |= steam_id == u64::from(u32::MAX) + 1;
            checked_wrapping_boundary |= steam_id == u64::MAX;
        }
        assert!(
            checked_byte_boundary,
            "native vectors must cover 8-byte LE input"
        );
        assert!(
            checked_wrapping_boundary,
            "native vectors must cover wrapping u64 arithmetic"
        );
    }
}
