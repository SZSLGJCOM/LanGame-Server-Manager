# Barotrauma campaign checkpoint control

This extension invokes the native `SaveUtil.SaveGame(GameSession.DataPath, false)`
once on the game owner thread. It saves an existing multiplayer campaign while
between rounds. The game does not expose a complete, non-destructive snapshot of
an arbitrary ongoing round: `SavePlayers` also clears inventories as part of round
teardown, and ending a round changes gameplay. Neither operation belongs to this
save action. A running round, absent campaign or pending submarine switch is
rejected with a specific reason.

Starting the instance automatically prepares the extension before creating the
game process. The stopped-instance **Prepare save extension early** action can
perform the same setup ahead of startup. LanGame publishes the fixed
source as an owned `.langame-world-save/LanGame.Barotrauma.Save.dll`; the normal
.NET startup-hook mechanism loads it on the next start. An existing nonempty
`DOTNET_STARTUP_HOOKS` is preserved as a conflict, rather than overwritten. No
external mod loader, console-command replacement or global setting is needed.
The adapter uses the shared authenticated, bounded `SaveProtocol.cs` and pinned
ordinary-file `SaveFiles.cs` from Core Keeper control.

## Native contract

The admitted installed server is 1.13.4.0, build string
`release/summer-update-2026-hotfix-1`, revision `6029cd01d7`. Both file hashes must
match before initialization:

- `DedicatedServer.dll`: `18278624996b543a2595a0ea9c0c4c14881d511ac307e1a69028d6579e67ad3c`
- `BarotraumaCore.dll`: `72acd77784b83a0e09584262ab1eea9ee40395b95f3a2e3ec3974da0bc0151b3`

The reviewed public [CrossThread implementation](https://github.com/FakeFishGames/Barotrauma/blob/ca7f5976fe16de7b153585ac4f6a12bdccbbbf8e/Barotrauma/BarotraumaShared/SharedSource/Utils/CrossThread.cs)
and [native save implementation](https://github.com/FakeFishGames/Barotrauma/blob/ca7f5976fe16de7b153585ac4f6a12bdccbbbf8e/Barotrauma/BarotraumaShared/SharedSource/Utils/SaveUtil.cs)
establish the intended boundaries; exact installed API/layout compatibility is
separately checked through reflection and real native acceptance. The public
reference is not claimed to be the installed binary's source commit.

`GameMain.Run` processes the native `CrossThread.enqueuedTasks` queue on its main
thread. The timer adds at most one native Task using its real delegate and
constructor. It takes the same queue lock with `Monitor.TryEnter`, skipping busy
ticks without accumulating blocked Timer callbacks. After native completion,
the next timer tick observes `Done` under that lock and disposes the task's
ManualResetEvent. A pending task remains alive through process exit, so the
native consumer never signals a disposed event. Stopping the channel prevents
the queued callback from invoking save logic.

This queue is independent of active console question prompts. Using a periodic
hidden console command would be unsafe because native `ExecuteCommand` treats
the next queued command as a prompt answer; this implementation never does that.

The owner callback verifies the actual `GameMain.MainThread`, current server and
session, non-running campaign and absence of submarine switching. It captures
the game's current DataPath, then rechecks session identity and round state
immediately before the native call. The IPC peer cannot choose a path or native
object. A durable dispatch marker precedes the call, and nonce reuse cannot
repeat it. Timeout or an exception after that marker is unknown, never a reason
for automatic resend.

`SaveGame` returns void and catches four classes of save failure. The extension
therefore observes `AppDomain.FirstChanceException` only for the request's owner
thread and only during that exact native invocation. Any exception, including
one swallowed by the game or raised by a stream's disposal, leaves the outcome
unknown. The observer is always removed before returning or propagating an
exception; another worker thread cannot acknowledge or reject the request.

The installed hash-pinned IL was checked through `SaveUtil.SaveGame`,
`GameSession.Save`, `SafeXML.SaveSafe`, `SubmarineInfo.SaveAs`,
`CompressStringToFile` and `CompressDirectory`. All early returns in this save
path follow exceptions. `GameSession.Save` passes `throwExceptions=true` and
`maxRetries=0`; a denied output stream produces an explicit exception. The
normal path reaches `CompressDirectory`, which closes the GZip and file streams.
Only this fixed native contract, an exception-free return, and a final check of
the same server, session, campaign and SavePath produce `status=completed`.
The desktop then records confirmed native completion. This is a campaign
checkpoint between rounds, with the live-round guards still enforced.

## Verification

The production source compiles with the existing Framework C# compiler using
`/langversion:5 /warn:4 /warnaserror+`. `BarotraumaControlTests.cs` compiles as an
external `DedicatedServer.exe` with the production adapter and shared protocol
sources. Its native boundary is synthetic; it verifies live-round/no-campaign/
submarine-switch rejection, current-session binding, exactly one checkpoint
invocation, swallowed and escaping I/O failures, unrelated-worker isolation,
observer cleanup, bounded task publication, nonblocking busy-owner behavior and
completed-event disposal. It does not replace real game acceptance.

Real acceptance used an ordinary copied server with a newly generated,
zero-player campaign, explicit private `config_player.xml` savepath and a real
Windows ConPTY. The production startup hook loaded successfully. No-campaign
and active-round requests were rejected. After the normal private round returned
to the lobby, one authenticated save request received a correlated dispatch
receipt and changed the current `.save` contents and modification time. A new
process subsequently loaded that saved campaign through the native campaign
loader without any manual save request. The test did not call teardown or
`SavePlayers` through the production save adapter.

The complete production lifecycle was also tested locally on 2026-10-19 with
test binary SHA-256
`6a607a97a1e4124f667ad3d9ba472734dc0b738052417034ec12a3d5b7f461ce`.
Automatic preparation and the startup hook passed on both managed runs. The
initial run created a fresh zero-player campaign through native console setup;
the restart selected and loaded the unique same fixture campaign without
recreating it. Both creation and loading start a native round, so this test-only
setup waited for the real round start, sent `endgame` once, and waited for native
round completion before attempting the formal save. Native `EndGame` clears
`GameSession.IsRunning` and `Server.GameStarted` before reporting completion;
the production save callback independently checks both again.

Each run made one formal authenticated save request, received one correlated
dispatch receipt, and stopped normally with exit code 0 and no crash flag. The
new managed run loaded the campaign successfully, and final independent OS
checks found no fixture server process or remaining game/query-port listener.
These two receipts retain pending disk completion: this full lifecycle test did
not separately establish durable writes or compare persisted campaign contents.
Campaign setup and `endgame` can save through the game's normal lifecycle; those
writes are not attributed to the manual action. An earlier restart attempt
correctly rejected its one formal request while the round was running, before
invoking `SaveGame`; the request was not retried.

Windows `Environment.GetFolderPath(LocalApplicationData)` is not reliably
redirected by `APPDATA`/`LOCALAPPDATA`. The installed self-contained .NET 8 runtime
was separately probed without loading game code: the test environment can return
an empty KnownFolder, making the game's fixed default save path relative. Native
`Program.Main` sets the working directory to the entry assembly directory. The
test permits only that exact relative default, verifies the primary executable
is in the fixture configuration directory, and otherwise requires an inspected
empty absolute fallback. It forces the actual `config_player.xml` `savepath`
inside the fixture before creating or loading a campaign; foreign campaigns and
unknown relative paths are refused.

On 2026-10-20 the completion-aware production adapter passed a new full local
acceptance with immutable test binary SHA-256
`f427c9250a5da3c7c92a49da92876a8e3fcce9364e75e82b73a20445f36d1893`.
The initial fixture held only its own campaign primary open without write/delete
sharing. Exactly one real save request reached `SaveUtil.CompressDirectory` and
failed on that primary's sharing violation; its new nonce/run/PID-correlated IPC
outcome was unknown and the pinned primary SHA-256 stayed unchanged. An admission
rejection, unrelated exception or success response could not pass this fault
check. The read handle was then released.

A fresh formal request in each of the initial and restarted managed runs received
`save_completion=confirmed` and `write_confirmation_pending=false`. Each native
server stopped normally with exit code 0 and no crash flag; the new managed run
loaded the existing fixture campaign. The runner exited 0 and independent OS
checks found no Sons or Barotrauma server process, no UDP listener on
8766/9700/27015/27016, and no TCP listener on those ports. The fixture had no player
client. This establishes request-correlated native checkpoint completion and
failure protection, not durable storage or comparison of player progress.
