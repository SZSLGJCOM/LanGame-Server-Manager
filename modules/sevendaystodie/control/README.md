# 7 Days to Die native manual save

The `save_world` action sends the native `saveworld` command once through the
server's Telnet interface. Enable `telnet_enabled`, configure `telnet_password`
and a valid TCP `telnet` port, then restart after configuration changes.

EAC remains the operator's existing setting and does not block this native
command. Saving no longer installs an `IModApi`/Harmony writer observer or
requires its fixed-build profile. Existing game Mods and anti-cheat settings
are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
