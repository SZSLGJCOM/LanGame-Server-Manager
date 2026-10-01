use sqlx::SqliteConnection;

use crate::StorageError;
use crate::instance_archive_store::{Snapshot, invalid};

// The snapshot is a bound JSON value. Listing never exercises INSERT triggers
// or recreates every historical row merely to determine recovery availability.
const RECORD_CONFLICT: &str = r#"SELECT CASE
WHEN EXISTS(SELECT 1 FROM instances WHERE id=json_extract(?1,'$.tables.instances[0].id')) THEN 'The original instance ID is already registered.'
WHEN NOT EXISTS(SELECT 1 FROM modules WHERE id=json_extract(?1,'$.tables.instances[0].module_id')) THEN 'The original module registration is missing.'
WHEN json_extract(?1,'$.tables.game_installs[0].scope')='instance' AND EXISTS(SELECT 1 FROM game_installs WHERE id=json_extract(?1,'$.tables.game_installs[0].id') OR owner_instance_id=json_extract(?1,'$.tables.instances[0].id')) THEN 'The original independent program ID or owner is already registered.'
WHEN EXISTS(SELECT 1 FROM instance_ports current JOIN json_each(?1,'$.tables.instance_ports') saved ON current.id=json_extract(saved.value,'$.id') OR (current.port=json_extract(saved.value,'$.port') AND current.protocol=json_extract(saved.value,'$.protocol'))) THEN 'An archived port or port record ID is already reserved.'
WHEN EXISTS(SELECT 1 FROM instance_runs current JOIN json_each(?1,'$.tables.instance_runs') saved ON current.id=json_extract(saved.value,'$.id')) THEN 'An archived run record ID is already registered.'
WHEN EXISTS(SELECT 1 FROM instance_broadcast_policies current JOIN json_each(?1,'$.tables.instance_broadcast_policies') saved ON current.instance_id=json_extract(saved.value,'$.instance_id')) THEN 'An archived broadcast policy is already registered.'
WHEN EXISTS(SELECT 1 FROM instance_broadcast_events current JOIN json_each(?1,'$.tables.instance_broadcast_events') saved ON current.event_id=json_extract(saved.value,'$.event_id')) THEN 'An archived broadcast event ID is already registered.'
WHEN EXISTS(SELECT 1 FROM json_each(?1,'$.tables.instance_runs') saved WHERE json_extract(saved.value,'$.status')='running') THEN 'Archive contains an active process record.'
ELSE NULL END"#;

pub(super) async fn check(
    connection: &mut SqliteConnection,
    snapshot: &Snapshot,
    original: &std::path::Path,
) -> Result<(), StorageError> {
    let problem: Option<String> = sqlx::query_scalar(RECORD_CONFLICT)
        .bind(serde_json::to_string(snapshot)?)
        .fetch_one(connection)
        .await?;
    match problem {
        Some(problem) => Err(invalid(original, problem)),
        None => Ok(()),
    }
}
