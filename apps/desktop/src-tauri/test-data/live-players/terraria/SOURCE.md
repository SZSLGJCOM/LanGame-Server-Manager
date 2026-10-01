# Terraria playing command

Synthetic names/addresses and protocol text from the installed official Terraria
Dedicated Server 1.4.5.6 `TerrariaServer.exe`, inspected as IL/resources on 2026-09-07.
No server or real client was started.

- `Terraria.Main.ReadLineInput` calls `Console.ReadLine`, so redirected stdin is supported.
- `Main.startDedInputCallBack` tests `CLI.Playing_Command`, loops active `Main.player`
  slots, and prints `<name> (<Clients[i].Socket.GetRemoteAddress()>)` for every active player.
- It then prints `CLI.NoPlayers`, `CLI.OnePlayerConnected` or `CLI.PlayersConnected`
  from the same loop's count. That footer is required before publishing a complete list.
- `LocalizedText.EqualsCommand` accepts both current localized value and `EnglishValue`:
  the fixed command `playing` works across server languages.
- `counts.txt` copies only those three protocol templates from each of the twelve shipped
  `Terraria.Localization.Content.<culture>.json` resources, substituting `2` for `{0}`.
  Names and parentheses are kept intact; remote addresses are discarded.
- The input loop prints `: ` without a newline before reading a command. A fresh stdout
  capture can therefore begin with that prompt; an unterminated footer is never accepted.
