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
assembly fingerprint before enabling the extension. Unknown builds deactivate
only an unchanged owned bridge and allow native game startup with manual save
unavailable. Directory reparse points, conflicting user DLLs and failed cleanup
still block startup. Its small ownership record allows only verified previous
LanGame payloads to be replaced or retired, and is retained for recovery.

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
The maintenance/card save uses a separate `save <nonce>` frame. The nonce is
exactly 32 lowercase ASCII hexadecimal characters: a 16-character Windows
`GetTickCount64` uptime in milliseconds followed by 16 random hexadecimal characters. Native
admission permits at most 10 seconds of queue age and 2 seconds of future clock
skew. The desktop and native bridge share this monotonic OS clock, so wall-clock
correction cannot revive expired requests. It consumes the nonce before checking world readiness or saving, retains
unexpired identities in a bounded replay set, and rejects expired/repeated
requests without invoking the save. The host never retries a sent request.

Only this frame emits `native_save_completed nonce=<nonce> bytes=<positive>`
after the synchronous native save returns and its new nonempty checkpoint
identity is verified. Readiness, native exceptions and checkpoint failures emit
`native_save_failed nonce=<nonce> code=<reason>` instead. These results describe
this checkpoint write; they do not assert physical-device flush guarantees.
The ordinary `save` and `shutdown` commands retain their original contract.

`commands_theforest_world_save.rs` pins the supported game assembly and the
exact embedded control DLL against replacement. Its owned blocking worker
holds the storage/mutation leases across the one stdin write and observation.
It requires the current run, session, primary PID and creation identity, an
instance-owned program path and a contiguous managed console ledger. It starts
at that ledger's current end and accepts only this request's exact terminal
line within one 60-second budget, rejecting gaps, malformed/duplicate results,
native failure and process changes. Command delivery alone never confirms a
save; an unknown outcome is reported without sending again.
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
