# SCUM player-query fixtures

`empty.json` is an actual nonce-matched response from an isolated Windows dedicated server on 2026-09-08: Steam app `3792580`, build `24973389`, UE `4.27.2`, and official UE4SS `3.0.1-1125-g527a483b`. No original saves or real client sessions were used.

`normal.json` is a constructed two-player protocol fixture with duplicate Chinese names and HTML-like text. It is not a populated-server capture. Numeric PlayerId values are session identifiers, not Steam64 IDs or moderation targets.

The independent producer reads current authoritative connections on the game thread and cross-checks the entire player-state array. Partial, stale or unreadable snapshots are rejected. See [the extension evidence](../../../../../../modules/scum/extensions/SOURCE.md) for source, loader hashes and actual verification boundaries.
