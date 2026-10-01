# Necesse players command

These are synthetic fixtures derived from the installed official Necesse Dedicated Server
build 23522725 `Server.jar`; they are not captures of real players.

Read-only class-file inspection on 2026-09-07 confirmed:

- `PlayersServerCommand.runModular`: counts non-null `Server.getClient(slot)` results;
  prints `Players online: <count>/<slots>`; loops the same slots and prints
  `Slot <slot + 1>: <authentication> "<name>", latency: <latency>, level: <level>,conn: <connection>`.
- Console requests (`serverClient == null`) use `ServerClient.getName`, whereas client
  requests use teleport markup. Only console output is accepted by this adapter.
- `CommandLog.print` writes the translated string to `System.out.println` with color codes
  removed. `GameLog.FileConsoleStream.formatString` adds `[yyyy-MM-dd HH:mm:ss] `;
  its console `print` prepends `FormatPrefix.WHITE`, ANSI `ESC[39m`.
- The count is the response boundary; absent rows, incomplete final lines, interleaved
  output, invalid slots, and a count mismatch are failures, not empty-player snapshots.

Only name and nonnegative latency are projected. Authentication, connection details,
and slot numbers are not account identities or row-action targets.

The official command reference distinguishes online `players` from historical
`playernames`: <https://necessewiki.com/Multiplayer>.
