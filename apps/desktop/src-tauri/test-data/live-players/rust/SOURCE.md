# Rust playerlist response contract

The normal and empty JSON fixtures are synthetic. Names, identifiers and the documentation-only address are test values, not a recording of players on a real server.

Protocol evidence inspected on 2026-09-07:

- Facepunch's WebRCON client requests `playerlist` and JSON-decodes `response.Message`: https://github.com/Facepunch/webrcon/blob/gh-pages/js/rconService.js
- Its player table reads `DisplayName`, `SteamID`, `Ping`, and `ConnectedSeconds`, and kicks using `SteamID`: https://github.com/Facepunch/webrcon/blob/gh-pages/html/playerlist.html and https://github.com/Facepunch/webrcon/blob/gh-pages/js/playerlist.js

The adapter projects only the displayed name, identity, ping and connection duration. It does not return the IP address or anti-cheat telemetry. Empty JSON arrays explicitly represent an empty list. Non-array, invalid, missing-identity and duplicate-identity responses fail collection.
