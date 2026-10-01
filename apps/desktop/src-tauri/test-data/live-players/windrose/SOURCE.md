# Windrose player snapshots

`empty.json` is an actual response from the isolated Windows Windrose build 24913903 server on 2026-09-08, running this repository's LgsmPlayerQuery with UE4SS 3.0.1-1125-g527a483b. The exact nonce, boot identifier and timestamp are retained; they are request correlation values, not credentials.

`normal.json` is a constructed protocol fixture for duplicate names and distinct session IDs. It is not a captured multiplayer session. PlayerId is session-scoped and is not a Steam account or a moderation target.

The producer and version-specific evidence are under `modules/windrose/extensions/`. The producer reads the current world's GameNetDriver.ClientConnections on the game thread and cross-checks the full PlayerArray. It rejects pending, inactive, ambiguous or unreadable connections and returns complete=false on failure. No Windrose+ cached status, character fallback, game saves or join/leave reconstruction is used.
