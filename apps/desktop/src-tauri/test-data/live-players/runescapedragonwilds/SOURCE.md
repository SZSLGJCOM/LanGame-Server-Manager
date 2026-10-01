# Dragonwilds player-query fixtures

`empty.json` is an actual response collected on 2026-09-08 from an isolated Windows dedicated server, Steam app `4019830`, build `24574222`, engine `5.6.1-232224+++dominion+live`. It used the independently authored `modules/runescapedragonwilds/extensions/LgsmPlayerQuery` script, official UE4SS `3.0.1-1125-g527a483b`, and the `version.dll` produced by the repository's fixed-source build script. No original server saves or player data were copied into the probe.

`normal.json` is a constructed protocol fixture with duplicate Chinese display names and HTML-like text. It is not a populated-server capture. PlayerId values are non-stable session identifiers; no account identity or moderation action is inferred.

The producer checks the current `L_World`, one authoritative `GameNetDriver`, bidirectional connection/controller/player-state links, the entire `PlayerArray`, and the game's read-only `IsPlayerReady` getter. Any incomplete or unreadable state rejects the whole snapshot. The native reader validates process identity, request nonce, age, completeness, count, unique session IDs, and bounded file I/O.

Loader provenance, actual verification boundaries and primary API sources are maintained in [the extension evidence](../../../../../../modules/runescapedragonwilds/extensions/SOURCE.md). Real populated sessions, join/leave/reconnect, and the EOS authentication meaning of readiness have not been verified.
