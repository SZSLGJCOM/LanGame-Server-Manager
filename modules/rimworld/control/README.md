# RimWorld Together native client-save requests

This module runs the third-party RimWorld Together server. Its native console
provides `forcesave <username>`; this is not a first-party RimWorld dedicated
server API. The public `save_world` action remains `rimworld_save`/`save` as a
closed manager operation.

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
