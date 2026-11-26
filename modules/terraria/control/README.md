# Terraria native manual save

The official Windows 1.4.6.1 package writes v329 worlds. A managed desktop
run set the clock to midnight, issued one save and captured revision 2 before
normal shutdown. A fresh managed process loaded that checkpoint and returned
midnight on `time`. Both processes exited normally. The managed save receipt
reports a request.
See the [1.4.6.1 evidence](../reference-fixtures/2026-11-25-vanilla-1461.json).

On this version the GNS UDP socket shares the game TCP port and listens on all
interfaces. LanGame disables GNS for a specific bind address or a TCP-only
instance. The native TCP listener continues to use the configured address.
LanGame writes `upnp=0` when automatic port forwarding is off and `upnp=1`
when it is on. An isolated vanilla server with `upnp=0` accepted the native
time query while its TCP and GNS UDP sockets listened on the same port.

The module's `save_world` action sends exactly one `save` to the current
managed `main` process. It keeps the server running and does not create a backup.

The selected Terraria or tModLoader server receives its native command on
managed stdin. Existing runtime and Mod choices are retained. Saving does not
install a startup hook, rewrite the executable or require the retired writer
observer profile.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
