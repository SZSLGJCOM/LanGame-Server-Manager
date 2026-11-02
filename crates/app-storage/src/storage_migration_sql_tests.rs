use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-migration-sql-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.clone(),
            database_path: root.join("fixture.db"),
            ..StoragePaths::default()
        };
        std::fs::create_dir_all(&root).unwrap();
        let pool = crate::storage_db::connect_pool(&paths).await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO modules(id,name,version) VALUES('fixture','Fixture','1')")
            .execute(&pool)
            .await
            .unwrap();
        for (id, path) in [(1, "first"), (2, "second")] {
            sqlx::query("INSERT INTO game_installs(id,module_id,install_root,install_state) VALUES(?1,'fixture',?2,'installed')")
                .bind(id).bind(path).execute(&pool).await.unwrap();
        }
        pool.close().await;
        Self { root, paths }
    }
    fn patch(&self, id: i64, before_path: &str) -> SqlPatch {
        let before = std::collections::BTreeMap::from([
            ("module_id".into(), json!("fixture")),
            ("install_root".into(), json!(before_path)),
            ("scope".into(), json!("library")),
            ("owner_instance_id".into(), Value::Null),
        ]);
        let mut after = before.clone();
        after.insert("install_root".into(), json!(format!("moved-{before_path}")));
        SqlPatch {
            table: "game_installs".into(),
            key_column: "id".into(),
            key_value: json!(id),
            before,
            after,
        }
    }
    async fn roots(&self) -> Vec<String> {
        let pool = crate::storage_db::connect_pool(&self.paths).await.unwrap();
        let values = sqlx::query_scalar("SELECT install_root FROM game_installs ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        pool.close().await;
        values
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-migration-sql-")
        );
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn commits_path_rows_atomically_and_replays_exactly_once() {
    let fixture = Fixture::new().await;
    let patches = [fixture.patch(1, "first"), fixture.patch(2, "second")];
    apply(&fixture.paths, &patches).await.unwrap();
    assert_eq!(fixture.roots().await, ["moved-first", "moved-second"]);
    apply(&fixture.paths, &patches).await.unwrap();
    assert_eq!(fixture.roots().await, ["moved-first", "moved-second"]);
}

#[tokio::test]
async fn later_changed_record_rolls_back_every_earlier_path_update() {
    let fixture = Fixture::new().await;
    let patches = [fixture.patch(1, "first"), fixture.patch(2, "unexpected")];
    assert!(apply(&fixture.paths, &patches).await.is_err());
    assert_eq!(fixture.roots().await, ["first", "second"]);
}

#[tokio::test]
async fn journal_cannot_choose_arbitrary_sql_identifiers_or_columns() {
    let fixture = Fixture::new().await;
    let mut patch = fixture.patch(1, "first");
    patch.table = "game_installs; DELETE FROM modules".into();
    assert!(apply(&fixture.paths, &[patch]).await.is_err());
    let mut patch = fixture.patch(1, "first");
    patch.after.insert("id".into(), json!(999));
    assert!(apply(&fixture.paths, &[patch]).await.is_err());
    assert_eq!(fixture.roots().await, ["first", "second"]);
}
