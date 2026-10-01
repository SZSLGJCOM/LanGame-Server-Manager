# Squad ListPlayers response contract

The normal and empty fixtures are synthetic, with invented names and IDs. They are not live-server captures.

Evidence inspected on 2026-09-07:

- Current SquadJS upstream parses `ListPlayers` records with `ID`, `Online IDs`, `Name`, `Team ID`, `Squad ID`, `Is Leader` and `Role`: https://github.com/Team-Silver-Sphere/SquadJS/blob/master/squad-server/rcon.js
- Its ID parser documents platform-tagged Steam/EOS IDs: https://github.com/Team-Silver-Sphere/SquadJS/blob/master/core/id-parser.js
- The active-list header is visible in an upstream transport issue: https://github.com/Team-Silver-Sphere/SquadJS/issues/249

The adapter requires the active-list header, treats header-only output as empty, retains spectators with an empty role, and rejects malformed or duplicate identities. Session IDs are displayed but do not authorize cached row actions. Kick/ban use a Steam or EOS identity, never a reused session index. This is an independent protocol implementation; no upstream implementation is copied.
