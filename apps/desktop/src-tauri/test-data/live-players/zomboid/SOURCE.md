# Project Zomboid players response contract

The normal and empty fixtures are synthetic and contain no captured player data.

On 2026-09-07 the installed dedicated-server distribution's `java/projectzomboid.jar`, Steam build 24909836, was inspected read-only. `zombie/commands/serverCommands/PlayersCommand.class` collects `UdpConnection.usernames` and uses the `Players connected (N): ` header, a newline (console) or ` <LINE> ` separator, and a `-` prefix for each username. No server settings, saves or player databases were inspected.

The adapter checks the declared count and preserves exact usernames. A username is a game-defined command target, not a Steam ID, and is marked non-stable. Only username-based kick, ban and add-to-whitelist actions receive a binding; Steam-ID and role-based commands do not.
