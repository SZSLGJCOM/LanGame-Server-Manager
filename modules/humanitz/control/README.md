# HumanitZ native manual save over RCON

The `save_world` action sends the game's native `save` command once over its
RCON interface. Enable `rcon_enabled`, configure `rcon_password` and a valid TCP
`rcon` port, then restart after configuration changes. The dedicated transport
preserves HumanitZ's native authentication and `Game saved!` acknowledgement
framing; it does not send an unsupported generic completion marker.

Saving no longer installs UE4SS, a proxy or the former native writer observer.
Existing game Mods, loaders and operator settings are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
