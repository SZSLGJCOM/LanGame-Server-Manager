# Satisfactory FRM player response

The populated fixtures are synthetic and model the upstream response contract.

- [FRM dedicated-server API](https://github.com/porisius/FicsitRemoteMonitoring/blob/main/docs/modules/ROOT/pages/dedicatedserver.adoc) documents `POST api/v1` with `function=frm` and `endpoint=getPlayer`, returning the JSON array directly. This read-only endpoint does not require an administrator token.
- [Player implementation](https://github.com/porisius/FicsitRemoteMonitoring/blob/main/Source/FicsitRemoteMonitoring/Private/Endpoints/World/PlayerLibrary.cpp) enumerates character Actors and sets `Online` from `IsPlayerOnline()`.
- [Response fields](https://docs.ficsit.app/ficsitremotemonitoring/latest/json/Read/getPlayer.html) include Actor `ID`, `Name`, and `Online`.

Only `Online=true` rows are projected. Offline characters are deliberately included in the fixtures to prevent a saved-character roster from becoming an online list. Actor IDs are used only to reject duplicate rows; they do not authorize moderation or claim stable account identity. Empty arrays and complete offline-only arrays are valid empty results. Missing online flags, malformed JSON, invalid online names and duplicate online Actor IDs fail collection.

On 2026-09-08, installed dedicated-server build 24656085 (CL 502094), SML 3.12.0 and FRM 1.5.3 loaded an instance-owned world. The game HTTPS extension returned `404 bad_function`. Enabling the native FRM HTTP autostart option and setting its port through the game's server API produced `200 []` from `GET /api/getPlayer` on the configured HTTP service.

The production collector then read those persisted options from the instance's `data/Saved/Config/WindowsServer/GameUserSettings.ini` and returned two distinct, complete, fresh empty snapshots. The world reported ready and stopped normally. This establishes real empty-server transport behavior; populated, duplicate-name and Unicode-name rows remain protocol fixtures. No game client was started for this verification.

The collector uses the configured HTTP service when native autostart is enabled; otherwise it uses the documented game HTTPS extension. Both requests use numeric loopback without proxies, redirects or administrator credentials. Compatible SML/FRM and a loaded world are required; LanGame does not install these extensions automatically.
