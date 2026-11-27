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
const COMPANION_UPDATE: (&str, &str) = (
    "8b39b613abc8de1f53248f085d36b9646af79e6a1f7028e3491354d6d7a82a02",
    "e04362c36714bb5e64d610eb05d94b29168829a5f52da600b09b303836ce53cb",
);

#[test]
fn barotrauma_save_admits_complete_packages_and_rejects_mixed_assemblies() {
    let game = super::super::profile("barotrauma").unwrap();
    let packages = [SUMMER, SECURITY_PATCH, COMPANION_UPDATE];
    for pair in packages {
        let mut files = native_files(game).unwrap();
        assert!(select_barotrauma_files(pair.0, pair.1, &mut files));
        assert_eq!(
            files,
            [
                ("DedicatedServer.dll", pair.0),
                ("BarotraumaCore.dll", pair.1)
            ]
        );
        let unchanged = files.clone();
        for other in packages {
            if pair != other {
                assert!(!select_barotrauma_files(pair.0, other.1, &mut files));
                assert_eq!(files, unchanged);
            }
        }
        assert!(!select_barotrauma_files(
            "unreviewed-server",
            pair.1,
            &mut files
        ));
        assert!(!select_barotrauma_files(
            pair.0,
            "unreviewed-core",
            &mut files
        ));
        assert_eq!(files, unchanged);
    }
}
