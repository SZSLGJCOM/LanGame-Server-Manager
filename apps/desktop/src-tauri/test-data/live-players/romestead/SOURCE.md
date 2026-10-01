# Romestead list command

Synthetic parser fixtures; no real-player data or server execution was used.

Read-only IL inspection on 2026-09-07 of installed official build 23662689 confirmed:

- `Server.dll`, `Server.Program.CommandChecker` explicitly handles redirected stdin
  using `Console.In.ReadLine` and queues each trimmed command to `BaseServer.CheckCommand`.
- `CandideServer.dll`, `CandideServer.Server.BaseServer.CheckCommand`, `list` branch:
  `There are <JoinedPlayers.Count> players online:` followed by each available player info:
  `<Character.Name> (<Item2.Id>) - <Character.Position> - <ConnectedPeer>`.
- `CandideServer.Server.Network.ConnectedPeer.ToString` prints
  `Peer <Id> - <Peer or Connection>`. The fixture position/endpoint values are synthetic;
  the parser validates the delimiters and discards these fields.
- Missing player info can omit a row while the header still counts the connection.
  The parser requires the full declared count and reports incomplete output in that case.

Only character names are projected. Numeric IDs, coordinates and endpoints are neither
account identities nor row-action bindings. The existing installation smoke evidence in
`modules/romestead/config-sources.toml` independently records accepted `list/save/stop`
through managed stdin, but does not establish moderation side effects against clients.
