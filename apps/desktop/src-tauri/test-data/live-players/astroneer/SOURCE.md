# ASTRONEER native player console

`normal.json` contains synthetic identities and names shaped after the native
`DSListPlayers` response. It includes online and historical players, non-ASCII
text and escaped quotes/braces. GUID values are opaque ASTRONEER identifiers;
they are never converted to Steam IDs or JavaScript numbers. `empty.json` is
the sanitized zero-player shape observed in an isolated dedicated server.

Protocol references:

- [AstroRCON transport](https://github.com/JoeJoeTV/AstroLauncher/blob/master/cogs/AstroRCON.py#L46):
  connect to local `ConsolePort`, send `ConsolePassword` plus LF, then
  `DSListPlayers` plus LF.
- [Online filtering](https://github.com/JoeJoeTV/AstroLauncher/blob/master/cogs/AstroDedicatedServer.py#L492):
  read `playerInfo`, filter `inGame`, and preserve `playerGuid` and `playerName`.
- [Configuration baseline](https://github.com/JoeJoeTV/AstroLauncher/blob/master/cogs/ValidateSettings.py#L100):
  client-selected default port 1234 and a generated console password.
- [Protocol field description](https://github.com/Esinko/AstroneerRconClient#command-reference):
  known-player records are distinct from currently connected players. This older
  client is supplementary documentation, not a current-version compatibility test.

On 2026-09-08, installed Steam app 728470 build 24411584 was copied to an isolated
temporary package with a fresh `Saved` tree, random ports and synthetic passwords.
An authenticated request returned `{"playerInfo":[]}` followed by CRLF. A wrong
password closed the connection without sending bytes. Authenticated shutdown
exited with code 0. No existing instance or real player data was read. Online
rows are fixture coverage, not evidence of a real joined-player smoke run.

The collector requires a complete JSON document and its CRLF terminator, bounds
the full connection to three seconds and the response to 512 KiB, and never
uses short TCP reads to infer completion. It only returns online identities;
category, index, historical records and moderation bindings are not projected.
