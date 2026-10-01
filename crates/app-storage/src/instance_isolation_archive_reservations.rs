use sqlx::SqliteConnection;

use super::{Layout, native};
use crate::StorageError;
use crate::instance_archive_store::{self as store, invalid};

/// Restoring journals reserve configuration and saves before a long copy starts.
/// Read under the caller's write transaction so admission and a competing path
/// change cannot both succeed. The restoring instance excludes its own claims.
pub(super) async fn read(
    connection: &mut SqliteConnection,
    instance_id: &str,
) -> Result<Vec<Layout>, StorageError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instance_archives WHERE state='restoring' AND instance_id<>?1",
    )
    .bind(instance_id)
    .fetch_one(&mut *connection)
    .await?;
    if count > store::MAX_ARCHIVES as i64 {
        return Err(invalid(
            std::path::Path::new(instance_id),
            "Pending archive path reservations exceed their limit.",
        ));
    }
    let mut layouts = Vec::new();
    // Recovery snapshots can each be large. Match the archive catalog's bounded
    // paging instead of materializing every pending snapshot in one allocation.
    for offset in (0..count).step_by(8) {
        let rows = sqlx::query(
            "SELECT * FROM instance_archives WHERE state='restoring' AND instance_id<>?1 \
             ORDER BY archive_id LIMIT 8 OFFSET ?2",
        )
        .bind(instance_id)
        .bind(offset)
        .fetch_all(&mut *connection)
        .await?;
        for row in &rows {
            let archive = store::map_archive(row)?;
            let snapshot = store::snapshot(&archive)?;
            let instance = store::instance(&snapshot);
            let id = store::string(instance, "id")?.to_owned();
            let module = store::string(instance, "module_id")?;
            let config = std::path::PathBuf::from(store::string(instance, "config_path")?);
            let root = config
                .parent()
                .ok_or_else(|| invalid(&config, "Archived configuration has no instance root."))?
                .to_owned();
            let install = snapshot.tables["game_installs"]
                .first()
                .ok_or_else(|| invalid(&root, "Archived program ownership is missing."))?;
            let runtime = std::path::PathBuf::from(store::string(install, "install_root")?);
            let saves = snapshot
                .effective_saves_path
                .as_deref()
                .unwrap_or(store::string(instance, "saves_path")?);
            layouts.push(Layout {
                id: id.clone(),
                name: store::string(instance, "name")?.to_owned(),
                root,
                config,
                saves: saves.into(),
                native_config: native::configuration_paths(module, &runtime, &id),
                runtime,
                mode: store::string(instance, "runtime_mode")?.to_owned(),
                issues: Vec::new(),
            });
        }
    }
    Ok(layouts)
}
