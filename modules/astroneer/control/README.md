# ASTRONEER native manual save

The `astroneer_save` action is a native TCP-console adapter, not a save plugin.
It requires the current owned dedicated process, a nonempty `console_password`
and the configured TCP `console` port. `DSListGames` and `DSServerStatistics`
must agree on a nonempty current world before submission.

The adapter sends password, `DSSaveGame` and a following read-only `DSListGames`
on the same connection. It keeps that socket alive through the complete query
reply because closing the peer can discard queued commands. This FIFO boundary
confirms command processing, not storage completion. It neither changes the
selected world nor loads a save.

Saving does not install UE4SS or the former native writer observer. Existing
startup-save selection, game configuration and loaders are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
