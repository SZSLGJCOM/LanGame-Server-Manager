# Barotrauma connected-client console response

- Server version: Dedicated Server 1.13.4.0, build 23799137.
- Command: `clientlist LGM_PLAYER_QUERY_<request_id>` through Windows ConPTY. The command ignores arguments and echoes the full submitted line.
- Source: [official server DebugConsole](https://github.com/FakeFishGames/Barotrauma/blob/master/Barotrauma/BarotraumaServer/ServerSource/DebugConsole.cs), `clientlist`, `UpdateCommandLine`, and `RewriteInputToCommandLine`.
- `empty.txt` preserves the verified isolated-server zero-client response structure after VT conversion. Its request ID is a fixed fixture correlation value.
- The production `app-runtime::spawn_launch_plan` path was exercised against a full isolated runtime copy: two distinct nonce queries returned complete empty responses and `exit` ended with code 0. The server rejected `save` as an unknown command.
- `normal.txt` is synthetic, derived from the official row format. No connected-player account or live player capture was used.
- Both delimiters and this request's exact command echo are required. Silence, previous request echoes, or a partial terminal row do not prove completeness.
- `display_name` preserves the native client label, which may include ` playing <Character.LogName>`. That suffix is unescaped and cannot safely be split from an arbitrary player name.
- Session IDs are temporary, read-only identifiers. The account and endpoint fields are parsed only as structural boundaries; this adapter creates no action targets.
