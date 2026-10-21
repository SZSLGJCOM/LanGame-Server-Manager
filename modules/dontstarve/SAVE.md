# Immediate native world-save requests

The `save_world` action uses the dedicated server's original `c_save()` command
through managed stdin. Every enabled shard must be running. LanGame validates
the enabled topology before sending and submits `c_save()` once to Master.
The native autosaver broadcasts the save to the other shards.

The manager no longer injects a `SaveCurrent` wrapper, serializes world data or
installs a manual-save observer. Native autosave timing, snapshot numbering,
world/player serialization and the game's own cross-shard behavior remain under
the game. Existing gameplay Mods and shard configuration are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.

This action does not create a LanGame backup or stop a shard. Normal stopping
uses its existing separate save-and-shutdown lifecycle; the manual-save change
does not replace that lifecycle.
