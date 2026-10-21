# ARK: Survival Evolved native manual save

The `save_world` action sends the original `saveworld` command through Source
RCON. Enable `rcon_enabled`, configure `admin_password` and the map's TCP `rcon`
port, then restart after configuration changes.

The manager validates every enabled running map before submission and sends
once per map. A later failure reports already-submitted and unattempted targets;
it does not replay earlier requests. Normal shutdown remains its separate
native save-and-exit sequence.

Saving no longer installs or calls `LgsmWorldSave.Save/Query` or a request-bound
writer-verification plugin. Existing ArkApi/AsaApi and LgsmArkTools used for other
operator features, game Mods and loader configuration are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
