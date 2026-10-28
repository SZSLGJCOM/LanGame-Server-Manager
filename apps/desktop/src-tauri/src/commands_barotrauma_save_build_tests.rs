#![cfg(test)]

use super::{native_files, select_barotrauma_files};

const SUMMER: (&str, &str) = (
    "18278624996b543a2595a0ea9c0c4c14881d511ac307e1a69028d6579e67ad3c",
    "72acd77784b83a0e09584262ab1eea9ee40395b95f3a2e3ec3974da0bc0151b3",
);
const SECURITY_PATCH: (&str, &str) = (
    "d058bbde56286699aa3ffcdd95e87d4d1e430b184ecc3c4007ea442ee9518a46",
    "48d637d505a2af5136feaafa871e5057e1f280a56f502d08e52f372816efe606",
);

#[test]
fn barotrauma_save_admits_both_complete_packages_and_rejects_mixed_assemblies() {
    let game = super::super::profile("barotrauma").unwrap();
    for pair in [SUMMER, SECURITY_PATCH] {
        let mut files = native_files(game).unwrap();
        assert!(select_barotrauma_files(pair.0, pair.1, &mut files));
        assert_eq!(
            files,
            [
                ("DedicatedServer.dll", pair.0),
                ("BarotraumaCore.dll", pair.1)
            ]
        );
    }
    for pair in [
        (SUMMER.0, SECURITY_PATCH.1),
        (SECURITY_PATCH.0, SUMMER.1),
        ("unreviewed-server", SECURITY_PATCH.1),
        (SECURITY_PATCH.0, "unreviewed-core"),
    ] {
        let mut files = native_files(game).unwrap();
        let unchanged = files.clone();
        assert!(!select_barotrauma_files(pair.0, pair.1, &mut files));
        assert_eq!(files, unchanged);
    }
}
