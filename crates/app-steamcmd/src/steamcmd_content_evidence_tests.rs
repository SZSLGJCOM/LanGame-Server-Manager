use super::*;
use std::io::Write;

const DENIED: &str = "[2026-10-15 18:42:29] CDepotDownloadMgr::BYldRequestDepotManifest(App: 896660, Depot: 896662, Manifest: 6497701753597013477, branch: ): Failed to get manifest request code, 'Access Denied'\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new(initial: Option<&[u8]>) -> Self {
        let root = crate::tests::unique_test_root();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        let fixture = Self(root);
        if let Some(initial) = initial {
            std::fs::write(fixture.path(), initial).unwrap();
        }
        fixture
    }
    fn path(&self) -> PathBuf {
        self.0.join("logs/content_log.txt")
    }
    fn append(&self, text: &[u8]) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
            .unwrap()
            .write_all(text)
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn only_complete_new_lines_can_supply_denials() {
    let fixture = Fixture::new(Some(DENIED.as_bytes()));
    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(b"unrelated new connection failure\n");
    assert!(evidence.finish().await.is_empty());

    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(DENIED.trim_end().as_bytes());
    assert!(evidence.finish().await.is_empty());

    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(b"\n");
    fixture.append(DENIED.as_bytes());
    assert_eq!(
        evidence.finish().await,
        [ManifestDenial {
            app: 896660,
            depot: 896662,
            manifest: 6497701753597013477,
        }]
    );
}

#[tokio::test]
async fn a_log_created_by_this_attempt_supplies_evidence() {
    let fixture = Fixture::new(None);
    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(DENIED.as_bytes());
    assert_eq!(evidence.finish().await.len(), 1);
}

#[tokio::test]
async fn replacement_truncation_and_same_length_rewrites_disable_evidence() {
    for mode in ["replacement", "truncation", "same-length", "regrow"] {
        let initial = "h".repeat(DENIED.len() + 100) + "\n";
        let fixture = Fixture::new(Some(initial.as_bytes()));
        let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
        match mode {
            "replacement" => {
                std::fs::rename(fixture.path(), fixture.path().with_extension("previous")).unwrap();
                // Preserve the old checkpoint and grow the replacement: only
                // file identity, not length/checkpoint, detects this rotation.
                fixture.append(initial.as_bytes());
                fixture.append(DENIED.as_bytes());
            }
            "truncation" => std::fs::write(fixture.path(), DENIED).unwrap(),
            "same-length" => {
                let text = format!("{}{}", DENIED, "z".repeat(initial.len() - DENIED.len()));
                std::fs::write(fixture.path(), text).unwrap();
            }
            _ => std::fs::write(fixture.path(), DENIED.repeat(5)).unwrap(),
        }
        assert!(evidence.finish().await.is_empty(), "{mode}");
    }
}

#[tokio::test]
async fn excessive_growth_and_unreadable_snapshot_cannot_trigger_recovery() {
    let fixture = Fixture::new(Some(b"previous\n"));
    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    fixture.append(&vec![b'x'; MAX_NEW_BYTES as usize]);
    fixture.append(DENIED.as_bytes());
    assert!(evidence.finish().await.is_empty());

    std::fs::remove_file(fixture.path()).unwrap();
    std::fs::create_dir(fixture.path()).unwrap();
    let evidence = ContentLogEvidence::before_spawn(&fixture.0).await;
    std::fs::remove_dir(fixture.path()).unwrap();
    fixture.append(DENIED.as_bytes());
    assert!(evidence.finish().await.is_empty());
}
