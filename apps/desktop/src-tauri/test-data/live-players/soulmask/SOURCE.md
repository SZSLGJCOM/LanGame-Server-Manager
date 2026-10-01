# Soulmask Echo player table

Verified on 2026-09-08 against Steam app 3017310, build 25117179, in an isolated Windows server with no clients. The installed server's native `help` names `List_OnlinePlayers` / `lp` and `Disconnect` / `dc`; `dc` closes the management connection and leaves the server running.

`empty.txt` preserves the observed zero-player table header; line endings are normalized to LF for Git. Tests restore the native CRLF framing. `normal.txt` contains constructed accounts, names, pawn IDs and positions, based on the table columns documented in the [upstream Echo client's parser](https://github.com/sibercat/SoulMask-Server-Manager/blob/main/Services/RconClient.cs). These rows are synthetic, not a capture of connected players.

The isolated native server verified `lp` followed by a fresh `LGM_PLAYER_QUERY_END_<32 hex digits>` command returns the table and an exact `<nonce> Not Found!` line. `dc` then returns normal EOF. The unknown command has no matching native action and supplies an explicit response boundary.

Native `help` produced three interactive pages. On one connection, `help` + nonce, `n` + nonce, and `n` + nonce returned `PAGE: 1 of 3`, `2 of 3`, and `3 of 3`, each followed by the matching nonce line. The nonce does not reset the query cursor. Tests construct multi-page player tables with this observed control footer. A multi-page table populated with real players has not been captured.

The implementation requires a table header, complete CRLF rows, every declared page in order, an exact fresh nonce line for each page, and final normal EOF. It never treats idle time, partial output, access lists, or historical players as a complete online roster. Existing manual Source RCON actions remain separate; no row actions are authorized by this read-only adapter.
