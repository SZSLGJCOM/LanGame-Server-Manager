use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use app_core::scum_custom_zones::{
    ScumCustomZoneConfiguration, ScumCustomZonesSnapshot, ScumCustomZonesStatus,
    ScumNpcVehicleDamage, ScumZoneHandling, WriteScumCustomZoneDamageInput,
};
use serde::Serialize;
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};

use super::io::{DatabaseAnchor, invalid};
use crate::StorageError;
use crate::instance_file_patch::sha256;

const CONFIGURATION_LIMIT: usize = 512;
const REGION_LIMIT: usize = 2048;
const DAMAGE_LIMIT: usize = CONFIGURATION_LIMIT * 32;
const NPC_ACTOR: i64 = 5;
const VEHICLE_DAMAGE_MASK: u64 = 0x3f0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Configuration {
    id: i64,
    map_id: i64,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Region {
    id: i64,
    map_id: i64,
    name: String,
    configuration_index: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Damage {
    id: i64,
    configuration_id: i64,
    map_id: i64,
    actor: i64,
    packed: i64,
    count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct NativeZones {
    configurations: Vec<Configuration>,
    regions: Vec<Region>,
    damage: Vec<Damage>,
}

pub(super) async fn connect(
    anchor: &DatabaseAnchor,
    write: bool,
) -> Result<SqliteConnection, StorageError> {
    anchor.check_auxiliary_files()?;
    // SQLite needs Windows' verbatim prefix for a long native save path. The
    // anchor still retains the original path for validation and error context.
    let filename = std::fs::canonicalize(&anchor.path)
        .map_err(|error| invalid(&anchor.path, error.to_string()))?;
    let options = SqliteConnectOptions::new()
        .filename(filename)
        .create_if_missing(false)
        .read_only(!write)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(2))
        .statement_cache_capacity(0);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    anchor.check_auxiliary_files()?;
    sqlx::query("PRAGMA trusted_schema=OFF")
        .execute(&mut connection)
        .await?;
    Ok(connection)
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    anchor: &DatabaseAnchor,
) -> Result<NativeZones, StorageError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *connection)
        .await?;
    if version != 73 {
        return Err(invalid(
            &anchor.path,
            "This SCUM database version has not been verified for custom-zone edits.",
        ));
    }
    let config_rows = sqlx::query("SELECT id,map_id,substr(COALESCE(name,''),1,1025) AS name FROM custom_zone_configuration ORDER BY map_id,id LIMIT ?1")
        .bind((CONFIGURATION_LIMIT + 1) as i64).fetch_all(&mut *connection).await?;
    let mut configurations = Vec::new();
    for row in config_rows {
        configurations.push(Configuration {
            id: row.try_get("id")?,
            map_id: row.try_get("map_id")?,
            name: row.try_get("name")?,
        });
    }
    let region_rows = sqlx::query("SELECT id,map_id,substr(COALESCE(name,''),1,1025) AS name,configuration_index FROM custom_zone_region ORDER BY map_id,id LIMIT ?1")
        .bind((REGION_LIMIT + 1) as i64).fetch_all(&mut *connection).await?;
    let mut regions = Vec::new();
    for row in region_rows {
        regions.push(Region {
            id: row.try_get("id")?,
            map_id: row.try_get("map_id")?,
            name: row.try_get("name")?,
            configuration_index: row.try_get("configuration_index")?,
        });
    }
    let damage_rows = sqlx::query("SELECT id,custom_zone_configuration_id,map_id,damage_actor_type,damage_handling_methods,num_damage_handling_methods FROM custom_zone_configuration_damage_handling_methods ORDER BY map_id,custom_zone_configuration_id,damage_actor_type,id LIMIT ?1")
        .bind((DAMAGE_LIMIT + 1) as i64).fetch_all(&mut *connection).await?;
    let mut damage = Vec::new();
    for row in damage_rows {
        damage.push(Damage {
            id: row.try_get("id")?,
            configuration_id: row.try_get("custom_zone_configuration_id")?,
            map_id: row.try_get("map_id")?,
            actor: row.try_get("damage_actor_type")?,
            packed: row.try_get("damage_handling_methods")?,
            count: row.try_get("num_damage_handling_methods")?,
        });
    }
    if configurations.len() > CONFIGURATION_LIMIT
        || regions.len() > REGION_LIMIT
        || damage.len() > DAMAGE_LIMIT
    {
        return Err(invalid(
            &anchor.path,
            "Custom-zone data exceeds the bounded editor limits.",
        ));
    }
    if configurations
        .iter()
        .any(|entry| entry.name.chars().count() > 1024 || entry.id < 1 || entry.map_id < 1)
        || regions
            .iter()
            .any(|entry| entry.name.chars().count() > 1024)
    {
        return Err(invalid(
            &anchor.path,
            "Custom-zone identifiers or names are not supported; the database was preserved.",
        ));
    }
    let configuration_keys: BTreeSet<_> = configurations.iter().map(|v| (v.map_id, v.id)).collect();
    if configuration_keys.len() != configurations.len() {
        return Err(invalid(
            &anchor.path,
            "Custom-zone configuration identities are ambiguous.",
        ));
    }
    let mut native_keys = BTreeSet::new();
    for row in &damage {
        if !configuration_keys.contains(&(row.map_id, row.configuration_id))
            || !native_keys.insert((row.map_id, row.configuration_id, row.actor))
        {
            return Err(invalid(
                &anchor.path,
                "Custom-zone damage rows are orphaned or ambiguous.",
            ));
        }
    }
    let result = NativeZones {
        configurations,
        regions,
        damage,
    };
    // Validate all three values before exposing any configuration as editable.
    for config in &result.configurations {
        result.vehicle_damage(config.map_id, config.id, anchor)?;
    }
    Ok(result)
}

impl NativeZones {
    fn npc_row(
        &self,
        map_id: i64,
        configuration_id: i64,
        anchor: &DatabaseAnchor,
    ) -> Result<&Damage, StorageError> {
        let row = self.damage.iter().find(|entry| entry.map_id == map_id && entry.configuration_id == configuration_id && entry.actor == NPC_ACTOR)
            .ok_or_else(|| invalid(&anchor.path, "This configuration has no persisted NPC damage row. Save its custom-zone settings in the current game before editing here."))?;
        if !(5..=32).contains(&row.count) {
            return Err(invalid(
                &anchor.path,
                "This NPC damage row does not contain the current five native slots. Save custom-zone settings in the current game before editing here.",
            ));
        }
        Ok(row)
    }

    fn vehicle_damage(
        &self,
        map_id: i64,
        id: i64,
        anchor: &DatabaseAnchor,
    ) -> Result<ScumNpcVehicleDamage, StorageError> {
        let bits = self.npc_row(map_id, id, anchor)?.packed as u64;
        let decode = |shift: u32| {
            ScumZoneHandling::from_native((bits >> shift) & 3u64).ok_or_else(|| {
                invalid(
                    &anchor.path,
                    "A vehicle-damage handling value is unknown; the database was preserved.",
                )
            })
        };
        // Verified native ArmedNPC list: slots 0/1 target players; slots 2/3/4
        // target Vehicle(11), with Melee(6), MeleeWeapon(5), Projectile(4).
        Ok(ScumNpcVehicleDamage {
            melee: decode(4)?,
            melee_weapon: decode(6)?,
            projectile: decode(8)?,
        })
    }

    pub fn revision(&self) -> Result<String, StorageError> {
        Ok(sha256(&serde_json::to_vec(self)?))
    }

    pub fn snapshot(
        &self,
        instance_id: &str,
        anchor: &DatabaseAnchor,
    ) -> Result<ScumCustomZonesSnapshot, StorageError> {
        let mut grouped = BTreeMap::<(i64, i64), Vec<String>>::new();
        for region in &self.regions {
            let configuration_id = region.configuration_index.checked_add(1).ok_or_else(|| {
                invalid(
                    &anchor.path,
                    "The region configuration index is invalid; the database was preserved.",
                )
            })?;
            grouped
                .entry((region.map_id, configuration_id))
                .or_default()
                .push(region.name.clone());
        }
        let mut configurations = Vec::new();
        for config in &self.configurations {
            configurations.push(ScumCustomZoneConfiguration {
                map_id: config.map_id,
                configuration_id: config.id,
                name: config.name.clone(),
                global: config.id >= 1000,
                regions: grouped
                    .remove(&(config.map_id, config.id))
                    .unwrap_or_default(),
                damage: self.vehicle_damage(config.map_id, config.id, anchor)?,
            });
        }
        Ok(ScumCustomZonesSnapshot {
            instance_id: instance_id.to_owned(),
            status: if configurations.is_empty() {
                ScumCustomZonesStatus::Empty
            } else {
                ScumCustomZonesStatus::Ready
            },
            database_identity: Some(anchor.identity.clone()),
            revision: Some(self.revision()?),
            writable: !configurations.is_empty(),
            configurations,
            message: None,
            backup_id: None,
        })
    }

    pub fn replacement(
        &self,
        input: &WriteScumCustomZoneDamageInput,
        anchor: &DatabaseAnchor,
    ) -> Result<(i64, i64, i64), StorageError> {
        if anchor.identity != input.database_identity || self.revision()? != input.expected_revision
        {
            return Err(invalid(
                &anchor.path,
                "Custom-zone data changed after it was loaded. Reload before saving.",
            ));
        }
        if !self
            .configurations
            .iter()
            .any(|entry| entry.map_id == input.map_id && entry.id == input.configuration_id)
        {
            return Err(invalid(
                &anchor.path,
                "The selected custom-zone configuration no longer exists.",
            ));
        }
        let row = self.npc_row(input.map_id, input.configuration_id, anchor)?;
        let current = row.packed as u64;
        let next = (current & !VEHICLE_DAMAGE_MASK)
            | (input.damage.melee.native_value() << 4)
            | (input.damage.melee_weapon.native_value() << 6)
            | (input.damage.projectile.native_value() << 8);
        Ok((row.id, row.packed, next as i64))
    }
}

pub(super) struct PreparedUpdate {
    before: NativeZones,
    row_id: i64,
    original: i64,
    next: i64,
}

impl PreparedUpdate {
    pub fn changed(&self) -> bool {
        self.original != self.next
    }
}

pub(super) async fn prepare(
    connection: &mut SqliteConnection,
    anchor: &DatabaseAnchor,
    input: &WriteScumCustomZoneDamageInput,
) -> Result<PreparedUpdate, StorageError> {
    let before = read(connection, anchor).await?;
    let (row_id, original, next) = before.replacement(input, anchor)?;
    let trigger: Option<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name='custom_zone_configuration_damage_handling_methods' LIMIT 1")
        .fetch_optional(&mut *connection).await?;
    if trigger.is_some() {
        return Err(invalid(
            &anchor.path,
            "Custom triggers on zone damage data are not supported; nothing was changed.",
        ));
    }
    let integrity: String = sqlx::query_scalar("PRAGMA quick_check(1)")
        .fetch_one(&mut *connection)
        .await?;
    if integrity != "ok" {
        return Err(invalid(
            &anchor.path,
            "The SCUM database failed its integrity check; nothing was changed.",
        ));
    }
    Ok(PreparedUpdate {
        before,
        row_id,
        original,
        next,
    })
}

/// The caller retains BEGIN IMMEDIATE across the pre-change instance backup,
/// this update and its commit. Only the existing row's three two-bit slots change.
pub(super) async fn apply(
    connection: &mut SqliteConnection,
    anchor: &DatabaseAnchor,
    input: &WriteScumCustomZoneDamageInput,
    prepared: PreparedUpdate,
) -> Result<ScumCustomZonesSnapshot, StorageError> {
    let PreparedUpdate {
        mut before,
        row_id,
        original,
        next,
    } = prepared;
    if original != next {
        let changed = sqlx::query("UPDATE custom_zone_configuration_damage_handling_methods SET damage_handling_methods=?1 WHERE id=?2 AND map_id=?3 AND custom_zone_configuration_id=?4 AND damage_actor_type=5 AND damage_handling_methods=?5")
            .bind(next).bind(row_id).bind(input.map_id).bind(input.configuration_id).bind(original)
            .execute(&mut *connection).await?;
        if changed.rows_affected() != 1 {
            return Err(invalid(
                &anchor.path,
                "The selected native damage row changed; reload before saving.",
            ));
        }
    }
    let after = read(connection, anchor).await?;
    let expected_row = before
        .damage
        .iter_mut()
        .find(|row| row.id == row_id)
        .ok_or_else(|| invalid(&anchor.path, "The selected native row disappeared."))?;
    expected_row.packed = next;
    if before != after
        || after.vehicle_damage(input.map_id, input.configuration_id, anchor)? != input.damage
    {
        return Err(invalid(
            &anchor.path,
            "The custom-zone write could not be verified; the transaction was rolled back.",
        ));
    }
    after.snapshot(&input.instance_id, anchor)
}
