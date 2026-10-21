# Soulmask immediate native persistence

The current local Windows server build is **25361728**. Its own Echo help,
captured from an ordinary isolated copy on 2026-10-19, lists:

```text
4: SaveWorld
Alia: sav
DESC: Only save the world, do not exit the game. Parameter 1 means force save and write to hard disk immediately.
```

The same native help lists `BackupDatabase` and `SaveAndExit` separately. This
current binary's contract replaces the earlier conflicting community advice
about `SaveWorld 0/1`. The forced command belongs to **Echo**, not the Source
RCON endpoint; the latter did not provide the save/help response in this build.

LanGame's closed `soulmask_save` transport checks the current managed process
PID, creation time, executable, local IPv4 destination and **all** competing TCP
listener owners. Binding the outgoing socket to the same local address prevents
an advertised remote address from becoming a control destination. It queries the
native help contract, leaves help pagination, and sends exactly one fixed
`SaveWorld 1`. An unpredictable exact native delimiter closes the bounded
response; idle gaps and short reads never count as complete. Ownership is checked
again before the sole save submission and after its response. The manual action
admits no arbitrary Echo command, backup, shutdown or automatic retry. Normal
shutdown uses its own closed native operation below.

The valid native save command has no per-save completion text. Its exact
delimiter establishes command processing only. After processing succeeds,
LanGame returns `save_completion=requested`; Maintenance and the card report
that the save request was sent and the game performs the save. This does not
confirm that all world data reached disk. Timeouts, lost ownership and
unexpected errors after submission remain errors with an unknown outcome;
the manager does not automatically repeat the request.

## Native evidence and limits

Private native probes used loopback game/RCON/Echo endpoints, random passwords,
fresh task-only `WS/Saved` and a 36,000-second automatic save/backup interval.
They captured the current help, ran one `SaveWorld 1` per probe run, retained the
exact command/nonce transcript and native command log, and verified SQLite
`integrity_check=ok`. The matching native response after the production sequence
was `Query exited.` followed by its exact nonce delimiter. The server stayed
running, and all probe processes were subsequently reclaimed.

These early command probes observed **zero persisted actors**. Their
32,768-byte database stayed byte-identical, so they do **not** claim a dirty
actor write or a causally confirmed save completion. A later fully initialized
zero-player server did generate and save native NPC actors; zero clients alone
does not imply an empty persistent world. The application preserves this
distinction in its result instead of using a nearby timestamp as success.

A later no-loader fixture observed 8,289 committed actor rows in a
25,419,776-byte snapshot before any stop, then restored that exact snapshot in a
new native process. Independent SQLite readers recovered the same actor keys
and complete row digest; both native stops exited with code 0 without forced
cleanup. This establishes usable committed data and native snapshot recovery.
It does not establish the end of every SaveWorld transaction: the game emitted
no save-finished receipt, and initial dungeon creation was still progressing
when the command ran. Initial native persistence has not been excluded as a
contributor. The application's manual-save action therefore reports native
request submission without claiming per-request persistence confirmation.

The in-game `gm BaoCun` publisher announcement is separate evidence of a player
administrator method, not the host transport used here:
[official Steam announcements](https://store.steampowered.com/news/posts/?enddate=1717360248&feed=steam_community_announcements).

## Native save and exit

Soulmask's own Echo help defines `SaveAndExit <seconds>` as saving and exiting
the game world after that countdown. LanGame declares exactly `SaveAndExit 1`
through `soulmask_echo_shutdown`; manual stop, restart and application shutdown
share the existing owned-process lifecycle. The full shutdown specification is
validated before I/O: one command, Echo only and no fallback. A lost/partial
response never causes another submission or a Ctrl+C fallback. The complete
managed process tree must exit before normal-stop finalization and any configured
automatic backup.

The native Echo target checks are shared with manual saving. A cold local
world's first help request had zero bytes after 8.23 seconds and completed after
12.79 seconds while its initial world generation occupied the game loop. The
same single help query now has a bounded 60-second response window; no command
is replayed. Native SaveAndExit may itself close Echo, so shutdown confirmation
uses the existing complete-process-tree wait instead of requiring the listener
to remain alive. Help and process-tree observation share one 90-second normal
shutdown allowance, within the existing 120-second final application-exit
deadline. The observed local native exit alone took 30.85 seconds, longer than
the former 15-second budget. An expired normal-stop allowance retains managed
ownership instead of forcibly stopping the server.

The shutdown path reuses the caller's existing storage lease, including after
application exit closes admission to ordinary work. Only target validation runs
in a blocking worker. TCP help and the sole shutdown write belong to the
cancellable async operation, so cancellation cannot leave a background sender.
The officially built manager passed local production start/stop acceptance on
2026-10-21. A second run with no other task game running again reached native
readiness, submitted the normal shutdown operation, observed native exit code 0,
left no active managed run and passed complete owned-process cleanup. Neither
run produced a manual-save receipt. This accepts the manager's native save-and-exit
path; running-world SaveWorld completion remains a separate unresolved boundary.

A separate no-loader local run submitted SaveAndExit once, recorded one native
command, `FDatabaseAccess::Shutdown` saving all actors and closing the world DB,
and exited with code 0 without forced cleanup. The native 32,079,872-byte DB had
11,155 actors and a successful SQLite integrity check. This certifies that
native save-and-exit experiment, not the still-pending nonstopping manual-save
completion and independent reload acceptance. No actor values were fabricated.

The native save and stop interfaces do not require an Unreal SDK or rebuilding
UE4SS. The following loader investigation concerns an optional completion
observer; it is not a prerequisite for hosting or native shutdown.

## Completion observer compatibility

The fixed dedicated executable has SHA-256
`dbae57f0f569f5ee94114ee180afab8e250117fba4827bd0920205a7408ff7a2`.
An isolated 2026-10-20 run with the reviewed UE4SS
`3.0.1-1125-g527a483b` terminated with `0xC0000005` during the loader
constructor, before Lua initialization. A separate physical copy with all 317
program files and launch arguments matched, but no loader, remained alive with
its own Echo and RCON listeners continuously present for 60 seconds. These are
startup diagnostics, not save or normal-shutdown acceptance.

All 39 import descriptors in this executable have `OriginalFirstThunk=0`.
The fixed core's [CMake dependency declaration](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/deps/third/CMakeLists.txt)
pins PolyHook `298d56210b9d9e66cde8f96481d6053925c6ae15`. Its
[original IAT lookup](https://github.com/stevemk14ebr/PolyHook_2_0/blob/298d56210b9d9e66cde8f96481d6053925c6ae15/sources/IatHook.cpp)
adds the module base before checking the OriginalFirstThunk pointer. The earlier
synthetic component used the xmake-pinned revision; this function is byte-for-byte
identical in both source revisions, but the CMake pin is the relevant provenance.

A fresh physical copy with the same fixed loader was then observed through the
standard Windows debug-event API. It produced a second-chance `0xC0000005` at
UE4SS.dll RVA `0xC1B2B2`; the original EXIT_PROCESS and independently queried
exit code matched. The core SHA-256 was
`91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`.
The exception lies in the unwind-linked function beginning at `0xC1B140`, at
`movzx ecx, byte ptr [rsi]`. Its control flow and three error-string references
match `FindIatThunkInModule`. The official same-build development package has
an identical core and a PDB whose GUID and age match its CodeView record.
Local symbolization independently confirms `PLH::IatHook::FindIatThunkInModule`
at `IatHook.cpp:125`. This establishes an actual import-name-walker fault,
rather than relying only on the synthetic reproduction. No register or target
memory dump was taken, so the particular module-base argument is not claimed.

The fixed constructor installs four LoadLibrary A/ExA/W/ExW IAT hooks without an
INI condition. They notify C++ mods about DLL loads; no supported setting has
been established that skips this initialization while retaining the required
save extension. The diagnostic changed no instructions, registers, imports,
protection settings or game assets, sent no Save command and reclaimed its owned
Job. It does not establish loader compatibility or save completion.

Consequently, this loader is not automatically installed for Soulmask. No
game headers, protection settings or encrypted assets are changed to make it
load. The native Echo delimiter and a valid SQLite integrity check still do
not establish this request's complete persistence result. A supported observer
must establish that causal write chain before this module can report confirmed
manual saves.

A separate official 2.5.2 core passed the constructor boundary but initially
failed during scanning with `bad conversion`. The fixed server's own numeric
version resource is 4.27.2.0 and its product string identifies UE4 Release 4.27.
In a fresh isolated copy, the supported `EngineVersionOverride` 4/27 setting
avoided that conversion error. All 60 scan attempts then ended with two missing
functions: `StaticConstructObject_Internal` and `FName::ToString`. No reflection
snapshot or Save call was produced, and the owned process tree was reclaimed.
The exact throwing instruction behind the earlier error remains unidentified.

The official configuration trees checked for the observed old core and a fixed
newer source revision supplied no Soulmask profile for these two functions.
The 2.5.2 candidate therefore remains a diagnostic result and is not installed
automatically. Neither a guessed signature nor a different game's profile can
establish compatibility with this pinned dedicated executable. Native hosting
and the accepted save-and-exit path remain available; running-world manual-save
completion has not passed acceptance.
