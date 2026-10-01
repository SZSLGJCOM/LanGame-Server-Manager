# The Forest managed control

The dedicated server's own `DllModLoader` loads this bridge from `DllMods`.
LanGame enables its entry with `LANGAME_THEFOREST_STDIN_BRIDGE=1` and supplies
the managed stdin pipe. It does not patch game assemblies or use a network
admin authentication bypass. Player moderation is not provided by this bridge.

The Windows desktop build invokes `scripts/build_theforest_control.ps1` with
the Windows .NET Framework C# compiler and embeds the resulting DLL. Only
platform BCL references are required; game and Unity DLLs are neither build
dependencies nor redistributed files. The game uses an older Mono BCL: the
build helper's `-RunTests` option checks emitted type/method references against
the known compatibility exclusions, including negative controls, and tests
input framing and checkpoint replacement. Its `-OutDir` must be outside the
source tree or in an approved build output directory.

`commands_theforest_control.rs` prepares the embedded DLL only in a verified
independent instance program directory. It verifies the supported game
assembly fingerprint and refuses unknown game builds, directory reparse
points, or conflicting user DLLs. Its small ownership record allows only
verified previous LanGame payloads to be replaced.

The native `saveFolderPath` requires a trailing separator because the game
concatenates the player mode directly. Startup refuses nonempty or non-plain
old `savesMultiplayer` / `savesSinglePlayer` sibling directories: back up and
restore those worlds beneath `saves/Multiplayer` / `saves/SinglePlayer` before
starting, and retain the old directory under a backup name. It never silently
moves or discards an existing world.

The bridge accepts `help`, `status`, `save`, and `shutdown`. Frames are bounded
ASCII lines with one queued command. Game APIs run on the Unity main thread.
Save requests require a loaded world and unsuspended serialization. The
bridge observes `DedicatedHost`, `FinishGameLoad`, and `IsSuspended` on the
Unity main thread, including idle frames. Its exact `world_ready=True` line
requires a loaded dedicated world, unsuspended serialization and a live owned
input reader; only state changes are logged, and losing a condition or starting
shutdown emits `world_ready=False`. Health and startup probes use these markers.
The native `Dedicated Server Running` banner precedes world completion and is
not readiness evidence. A `status` reply with additional fields is not a marker.
The native save call must create a nonempty checkpoint with a new Windows file
identity while the previous checkpoint remains pinned against identity reuse.
Timestamps and an existing recent save are not accepted as proof.
Metadata handles have one Win32 owner. The bundled Mono's
`FileStream.SafeFileHandle` getter creates a second owning wrapper without
transferring ownership; the bridge must not use it across the native save's
garbage collections.

The input reader checks the owned pipe for available bytes and waits on a
cancellation signal when idle. `shutdown` performs the save, cancels input and
confirms that the reader has exited before starting the game's own shutdown
coroutine. It never relies on closing a raw handle underneath a blocked read.
The native control log acknowledges requests and completed saves;
only LanGame's complete owned-process-tree exit check establishes successful
stopping. The isolated native lifecycle fixture exercises the production CMD
route, current-run acknowledgements, checkpoint changes, and final exit.

The 2026-10-01 isolated build-3488796 run on source `899e30c927e4` passed
`help`, `status`, manual save, a 30-second observation and shutdown save;
the original process exited with code 0 in 2454 ms. The corresponding run
identity and complete owned-tree exit were checked. This does not substitute
for existing-instance UI or multiplayer/client verification.

The final existing-instance run 130 on exported source `453921bdccc0` verified
the corrected world-ready signal and a 30-second stable observation. Commands
entered through the visible LanGameCMD input returned fresh `help`, `status`
(`world_ready=True serialization_suspended=False`) and `save` acknowledgements;
the manual checkpoint contained 196240 bytes. Normal shutdown completed in
2688 ms with exit code 0 and the matching run identity. No independent native
window was visible. The 647-line event transcript matched the persisted log;
all three stopped UI snapshots exactly matched the same source's retained
400-line tail, with no read failures across 36 UI samples. Multiplayer/client
and save-restart recovery are outside this start-stop verification.
