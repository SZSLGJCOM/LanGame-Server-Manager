# SCUM custom-zone vehicle damage

The Configuration workspace exposes three independent rules for each existing
zone configuration: NPC melee, NPC melee weapon and NPC projectile damage to
vehicles. Stop the instance before editing. Region creation, geometry and
player damage remain owned by the game.

The editor binds each inspected executable to its verified database schema:
build 25803875 to schema 73, build 26320224 to schema 75, builds 26593814 and
26621903 to schema 77, and build 26790009 to schema 79. Crossed pairs and
unknown versions are rejected. Let the game perform its database migration
before stopping it to edit zones. Paths come from the stored instance.
Writes retain the instance mutation and storage leases, acquire
SQLite's writer reservation, compare the displayed database identity and zone
revision, and create a normal save backup before changing three two-bit slots.
Unknown fields and the other damage slots are preserved. The committed values
are read back before success is returned. Maintenance provides the pre-change
backup for recovery.

Backups retain the main SQLite database and its committed WAL while the writer
reservation is held. Only `SaveFiles/SCUM.db-shm` is omitted: SQLite reconstructs
this transient WAL index, which carries Windows byte-range locks, on opening.
See the [SQLite WAL file format](https://www.sqlite.org/walformat.html).

`native-contract.json` records both isolated native registry observations and
read-only package analysis. The schema-75 region table definitions, columns,
foreign keys, indexes and triggers match the schema-73 contract. `fixture.sql`
is a synthetic schema-73 database; the tests also exercise its region
tables with the verified schema-75 and schema-77 versions. It includes unrelated data for
retention and backup regressions. Neither artifact contains user accounts or
proprietary game assets. The native probe establishes slot identity;
it does not establish client combat outcomes or a new full game lifecycle test.

The 2026-11-12 build generated schema 77 with the same required custom-zone
columns and two configurations/four regions. Its focused native database and
configuration evidence is in
[`2026-11-12-native_contract_build_26593814.json`](../reference-fixtures/2026-11-12-native_contract_build_26593814.json).
The official overlapping-zone priority fix belongs to the game's damage
resolution; it does not introduce an extra editor field. The exact executable
selects schema 77, and regression cases reject 73/75/76/78 before backup or
mutation. Native registry and public write validation are recorded separately
from this generated-world observation.

The new image's fixed-array registration and live property metadata agree on
16 actor entries, a 16-byte actor stride and a three-byte channel/receiver/handling
pair. The current-world registry identifies ArmedNPC 5 and Vehicle 11; the five
NPC pairs retain vehicle slots 2/3/4 and channels 6/5/4. This was confirmed with
read-only reflection and process-memory reads, including native enum values.
The observation reached readiness but direct Ctrl+C returned `0xC000013A` and
owned descendants required Job cleanup. It is not normal-shutdown evidence.

The opt-in storage test `native_schema_roundtrip_on_owned_copy` accepts
`LANGAME_SCUM_ZONE_NATIVE_ROOT` for an explicitly selected stopped disposable
world. It copies the native database/WAL and verified executable into a fresh
instance, exercises the public read/write API and pre-change backup readback,
and verifies that the original database and WAL bytes stayed unchanged. It never
starts a game process. The 2026-10-30 run passed both build/schema pairs, each
with two native configurations, and retained both original DB/WAL hashes.
On 2026-11-12 the new schema-77 world also passed this public API roundtrip,
including its two configurations, committed reload, pre-change backup and
unchanged source DB/WAL. All nine storage regressions passed across the three
supported synthetic layouts. These checks do not claim an in-game reload of
edited damage rules or client combat outcomes.

On 2026-11-14 build 26621903 retained schema 77 and the same live registry,
enum values, fixed-array metadata and vehicle slots. Its new exact executable
hash selects the existing verified layout. All nine SQLite regressions and the
public API roundtrip on a consistent native-world snapshot passed on an isolated Windows validation host;
source database and executable hashes remained unchanged. The portable test
accepts `LANGAME_SMOKE_SOURCE_ROOT` for its verified source snapshot, so module
discovery does not depend on another machine's compiled-in checkout path.
The [current contract](../reference-fixtures/2026-11-14-native_contract_build_26621903.json)
records the evidence and its separate managed lifecycle result.

On 2026-11-22 build 26790009 (`1.3.6.2.179673`) bumped `SCUM.db` to `PRAGMA user_version = 79`
while keeping identical custom-zone table DDL, indexes, zero triggers, two configurations,
four regions, and ArmedNPC(5) 5-slot damage rows (`341` / `682`). Live read-only UE4SS reflection
and process-memory observation on an isolated Windows validation host confirmed all eleven owner
and readiness checks, unchanged enum values, and fixed-array metadata matching the build 26790009
static registration (`array_dim = 16`, `offset = 88`, outer size `16`, inner pair size `3`,
`ArmedNPC(5)` -> `Vehicle(11)` slots `2/3/4`). Its exact executable hash selects
`NativeLayout::Build26790009` (schema 79); all nine SQLite regressions (spanning schemas
73/75/77/79 and crossed-pair rejections 77/79/78/80) and `native_schema_roundtrip_on_owned_copy`
on a consistent Backup API snapshot of the build 26790009 schema-79 world passed with unchanged
source files. See [`2026-11-22-native_contract_build_26790009.json`](../reference-fixtures/2026-11-22-native_contract_build_26790009.json)
and the separate 4-cycle managed lifecycle record
[`2026-11-22-managed_lifecycle_build_26790009.json`](../reference-fixtures/2026-11-22-managed_lifecycle_build_26790009.json).
