# Minecraft native manual save

The module's `save_world` action sends exactly one `save-all flush` to the current
managed `main` process. It keeps the server running and does not create a backup.

The selected server runtime receives the command on its managed stdin.
Existing vanilla, Mod-loader and Java configuration is retained. LanGame does
not add a Java agent or native writer observer for saving.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
