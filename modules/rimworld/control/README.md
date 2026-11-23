# RimWorld Together native client-save requests

This module runs the third-party RimWorld Together server. Its native console
provides `forcesave <username>`; this is not a first-party RimWorld dedicated
server API. The public `save_world` action remains `rimworld_save`/`save` as a
closed manager operation.

The pinned [26.11.21.1 (1) release](https://github.com/RimWorld-Together/Rimworld-Together/releases/tag/26.11.21.1_%281%29)
requires **RimWorld 1.7 unstable**, the matching Together client mod (Workshop
3005289691) and Harmony (2009463077). Players on 1.6 need the
[26.10.15.1 release](https://github.com/RimWorld-Together/Rimworld-Together/releases/tag/26.10.15.1).
Upstream notes recommend backing up existing servers before upgrading to
26.11.21.1 because player saves and user backups now use player hashes instead
of raw usernames. The hotfix also changes how guild membership is looked up.

The market button uses `Configs/Actions/Market.json`. `IsEnabled` controls
access from the game UI. `UseGlobalServer` selects the public cross-server
market or the instance's own market. Players connect directly to the public
service on outbound TCP 7790 when enabled. Local prices apply only to the
instance's market. New instances use the upstream public-market default.
Existing local markets continue locally unless the operator enables cross-server trading.
Servers older than 26.11.2.1 use their local market. An installed server can
keep running with its matching client when the catalog offers a newer release.
On 26.11.2.1 a non-admin seller could hold two local listings with
`MaxEntriesPerPlayer` set to one; tagged 26.11.21.1 `PM_Market.cs` retains the
same `OwnerHash == Username` comparison.

The player save rollback setting maps to `Configs/Actions/Rollback.json` →
`IsEnabled`. The matching client checks this flag when the player clicks the
rollback button. Confirming loads the player's last local save. Instance backup
and restore use their existing server-side operations.

Before updating an older instance with live listings, keep its program pinned
and have sellers withdraw their goods on the old server. The old
`Assets/Market.json` contains usernames and item labels. The new server reads
per-listing files with owner hashes and item definition names under
`Assets/Market/`. LanGame leaves the original file intact and checks it before
publishing the update.

The manager uses the current managed stdin/log channel to query `list` and
`deeplist`, rejects a server with no connected clients, and submits one native
`forcesave` command per validated registered username. RimWorld Together itself
skips offline clients. Offline worlds cannot be saved by this operation, and
command delivery does not establish a client's later save or upload.

No .NET startup-hook save observer is installed. The RimWorld Together client
Mod and existing server authentication, permissions and Mod configuration remain
unchanged.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.

The command contract is in the upstream release source:
[CMD_ForceSave](https://github.com/RimWorld-Together/Rimworld-Together/blob/26.8.31.1/Source/Server/Commands/CMD_ForceSave.cs).
