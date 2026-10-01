# Conan Exiles listplayers response contract

The normal and empty fixtures are synthetic and include no captured player information.

The installed `ConanSandboxServer-Win64-Shipping.exe`, Steam build 24922388, was inspected read-only on 2026-09-07. Its static strings include the six-column header and aligned format strings for `Idx`, `Char name`, `Player name`, `User ID`, `Platform ID`, and `Platform Name`. The independently maintained ServerManagers implementation documents a matching response example: https://git.tribufu.com/tribufu/ServerManagers/src/commit/aee6a394fe52da93be12ff32f12bbb7d90cc277c/src/ConanServerManager/Lib/ServerRcon.cs

The adapter binds `kickplayer userid` and `banplayer userid` to the game-defined User ID, not to the display name or platform ID. A header-only response is empty. Rows with missing columns, ambiguous pipe-delimited names, invalid identities or skipped indices fail collection. Unknown platform labels remain display attributes and are never relabelled as Steam or EOS IDs.
