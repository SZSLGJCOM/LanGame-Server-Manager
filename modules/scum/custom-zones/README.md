# SCUM custom-zone vehicle damage

The Configuration workspace exposes three independent rules for each existing
zone configuration: NPC melee, NPC melee weapon and NPC projectile damage to
vehicles. Stop the instance before editing. Region creation, geometry and
player damage remain owned by the game.

The editor accepts only the inspected build 25803875 executable and database
schema 73. Its paths come from the stored instance, never a caller-provided
database path. Writes retain the instance mutation and storage leases, acquire
SQLite's writer reservation, compare the displayed database identity and zone
revision, and create a normal save backup before changing three two-bit slots.
Unknown fields and the other damage slots are preserved. The committed values
are read back before success is returned. Maintenance provides the pre-change
backup for recovery.

Backups retain the main SQLite database and its committed WAL while the writer
reservation is held. Only `SaveFiles/SCUM.db-shm` is omitted: SQLite reconstructs
this transient WAL index, which carries Windows byte-range locks, on opening.
See the [SQLite WAL file format](https://www.sqlite.org/walformat.html).

`native-contract.json` records the isolated native registry observation and
read-only package analysis. `fixture.sql` is a synthetic SQLite database schema
with unrelated data for retention and backup regressions. Neither contains user
accounts or proprietary game assets. The native probe establishes slot identity;
it does not establish client combat outcomes or a new full game lifecycle test.

The opt-in storage test `native_schema_roundtrip_on_owned_copy` accepts
`LANGAME_SCUM_ZONE_NATIVE_ROOT` for an explicitly selected stopped disposable
world. It copies the native database/WAL and verified executable into a fresh
instance, exercises the public read/write API and pre-change backup readback,
and verifies that the original database and WAL bytes stayed unchanged. It never
starts a game process. The 2026-10-28 run covered two native configurations.
