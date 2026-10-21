# Romestead native manual save

The module's `save_world` action sends exactly one `save` to the current
managed `main` process. It keeps the server running and does not create a backup.

The dedicated server receives its native command on managed stdin. LanGame
does not install a .NET startup-hook save observer. Existing game and loader
configuration is retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
