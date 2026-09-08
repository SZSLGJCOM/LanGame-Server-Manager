# Games and capabilities

[简体中文](GAMES.md) | [English](GAMES.en.md) · [Back to product overview](README.en.md) · [LanGame website](https://langame.cn)

LanGame Server Manager (LGSM) provides server configuration for the following 32 games. This table lists examples of native settings, online player queries, and player management options to help server owners choose the coverage they need.

“List actions” operate on a player returned by an online query. “Manual” actions require the username or account ID accepted by the game. “Access lists” are persistent records for administrators, allowed or banned players, and priority access. Access lists are separate from current online players; changes take effect according to each game's rules.

## Complete game list

| Game | Example settings | Online player queries | Player management and requirements |
| --- | --- | --- | --- |
| 7 Days to Die | World type and seed, sandbox code, player and reserved slots | Names and current session IDs; read only | Manual kick, ban (10 years), and unban; admin, allow, and ban lists. Queries and immediate actions require Telnet enabled with a password. |
| Abiotic Factor | Difficulty, day/night speed, loot respawn | A2S list; read only, subject to server disclosure | Admin list; no list actions. |
| ARK: Survival Ascended | Map, taming speed, harvesting and XP multipliers | Names and account IDs returned by the server | List kick and ban; manual unban and no-check join management; admin, exclusive-join, and priority lists. Queries and immediate actions require RCON and an admin password. |
| ARK: Survival Evolved | Map, taming speed, harvesting and XP multipliers | Names and account IDs returned by the server | List kick and ban; manual unban; admin, exclusive-join, and priority lists. Queries and immediate actions require RCON and an admin password. |
| ASTRONEER | Active save, autosave interval, server frame rate | Names and Astroneer GUIDs; read only, limited to players currently in game | Queries require the native TCP console port and a nonempty password; no player management actions. |
| Barotrauma | Voice chat, karma, respawn shuttle, round restart | Client descriptions and latency; read only | Admin list. Queries require the LGSM managed console; no list actions. |
| Conan Exiles Enhanced | PvP and building damage rules, day/night speed, harvesting multiplier | Names and internal Conan UserIDs | List kick and ban; manual unban. Queries and immediate actions require RCON enabled with a password. |
| Core Keeper | World slot, seed, mode, season | A2S list; read only, subject to server disclosure | Admin and ban lists; no list actions. |
| Don't Starve Together | Game mode, surface and cave presets, seasons, resources | Names and Klei IDs on the Master shard | List kick; admin, allow, and ban lists. Queries and kicks use the managed Master console; this list does not cover all players on the Caves shard. |
| Enshrouded | Difficulty preset, health and stamina, shroud time | A2S list; read only, subject to server disclosure | Ban list; no list actions. |
| HumanitZ | PvP, death penalties, territory rules | Names; read only | Manual kick; admin, allow, ban, and reserved-slot lists. Queries and immediate actions require RCON enabled with a password. |
| Minecraft Java Edition (vanilla) | Game mode, difficulty, seed, view and simulation distances | Names and UUIDs; read only | Manual kick, ban, unban, OP, and whitelist management; operator, allow, and ban records. Queries and immediate actions require RCON enabled with a password. |
| Necesse | World name, player limit, pause when empty, seasonal content | Names; read only | Manual permission queries and changes, ban, and unban; owner configuration. Queries and immediate actions require the LGSM managed console. |
| Nightingale | Starting difficulty, player limit, passwords | Names and player count; read only | Queries require the HTTP status endpoint enabled; no player management actions. |
| Palworld | Day/night speed, XP and capture rates, death penalty | Names, platform user IDs, latency; read only | Manual kick, ban, and unban by Steam ID. Lists require the REST API and an admin password; immediate actions separately require RCON enabled. |
| Project Zomboid | PvP, registration and account rules, native sandbox Lua | Online usernames | List kick, ban, and whitelist add; manual Steam ID ban, user unban, access-level changes, and whitelist removal. Queries and immediate actions require an RCON password. |
| The Lord of the Rings: Return to Moria | World type and seed, combat difficulty, mining drops | No live list | No player management actions. |
| RimWorld Together | Player limit, whitelist, local save synchronization, server visibility | No live list | Manual kick, ban, unban, whitelist, and OP management by username; whitelist configuration. Uses the Together server; clients need its mod and dependencies. |
| Romestead | World name, size and seed, automatic creation and loading | Names; read only | Queries require the LGSM managed console; no player management actions. |
| RuneScape: Dragonwilds | Server and world names, world password, admin password | No live list | No player management actions. |
| Rust | Map, seed and world size, game mode, save interval | Names and Steam IDs | List kick; owner/moderator, ban, and priority lists. Queries and immediate management require Web RCON enabled with a password; priority-list changes take effect after restart. |
| Satisfactory | Player limit, network tick rate, rotating autosave count | Vanilla player count; an online list with a compatible FRM extension, read only | Extension queries use the authenticated server HTTPS API; no player management actions. Server claim, name, passwords, and active save remain in the in-game Server Manager. |
| SCUM | World and damage rules, economy and traders, raid windows | A2S list; read only, subject to server disclosure | Admin list; no list actions. |
| Sons of the Forest | Game mode, save slot, enemy strength, structure damage | A2S list; read only, subject to server disclosure | Owner list; no list actions. |
| Soulmask | PvE/PvP, save and backup intervals, XP multiplier | Names and Steam IDs; read only | Manual kick, ban, and unban. List queries require the native Echo TCP management interface and password; manual actions use the configured RCON endpoint and password. |
| Squad | Map and layer rotation, voting, faction exclusions, reserved slots | Names and EOS IDs | List kick and ban; manual kick and ban by player ID; admin and reserved-slot lists. Queries and immediate actions require an RCON password. |
| Terraria | World size, seed, difficulty, Journey Mode permissions | Online character names; read only | Manual kick and ban; ban list. Queries and immediate actions require the LGSM managed console. |
| The Forest | Difficulty, save slot, tree regrowth, building destruction | A2S list; read only, subject to server disclosure | No player management actions. |
| Unturned | Map, difficulty, perspective, PvE, Workshop content | A2S list; read only, subject to server disclosure | Manual admin management, kick, ban, unban, and join permits; admin list. Immediate actions require the LGSM managed console. |
| Valheim | World presets and modifiers, crossplay, save and backup intervals | A2S list; read only, subject to server disclosure | Admin, permitted-player, and ban lists; no list actions. |
| V Rising | PvE/PvP, castle damage, resource yield, death-container permissions | A2S list; read only, subject to server disclosure | Admin and ban lists; no list actions. |
| Windrose | Invite code, world presets, co-op quests, enemy and ship multipliers | No live list | No player management actions. |

## Scope to consider

- **Query protocols and player identities differ.** A2S requires an available query port and a server that publishes player names. Game versions, crossplay settings, and response formats affect the result. A2S names are not account IDs and cannot directly identify a ban target. ARK Steam, Epic, and EOS IDs and Conan internal UserIDs are handled separately.
- **Configuration and live management are separate capabilities.** An editable admin password, whitelist, or server rule does not imply immediate moderation or a live player list. Read-only rows do not expose management buttons; manual actions require the identity format accepted by that game.
- **Edition and mod coverage is specific.** Minecraft coverage is for vanilla Java Edition, with no compatibility promise for Bedrock Edition or arbitrary third-party servers. RimWorld coverage is for RimWorld Together, not other multiplayer mods. The Satisfactory list extension is Ficsit Remote Monitoring (FRM).
- **World settings follow the game's own activation rules.** Seeds and world-generation options generally affect new worlds or newly generated terrain. Related 7 Days to Die sandbox rules use a code generated by the game; Project Zomboid supports editing native sandbox Lua. Configuration coverage does not mean every rule in an existing save can change immediately.

Visit [langame.cn](https://langame.cn) for LGSM and LanGame OS, or [return to the LGSM product overview](README.en.md).
