# ARK ListPlayers response contract

All fixtures are synthetic; no live player data was collected. The adapter implements the numbered `index. name, account ID` response and the explicit `No Players Connected` response in `empty.txt`.

Evidence inspected on 2026-09-07:

- The ARK Remote Admin author's RCON guide includes numbered ListPlayers and empty response examples: https://steamcommunity.com/sharedfiles/filedetails/?id=479837541
- ARK's official community wiki distinguishes ASE Steam IDs (17 digits), ASE Epic account IDs (19 digits), and ASA EOS account IDs (32 alphanumeric characters): https://ark.wiki.gg/wiki/Server_configuration
- Current upstream ARK Server Tools also reads numbered `listplayers` records for crossplay servers: https://github.com/arkmanager/ark-server-tools/blob/master/tools/arkmanager

Names are split from the final comma, retaining commas in display names. The response must have a contiguous zero-based sequence. Only kick and ban actions bind account IDs; access-list and offline operations retain their separate contracts.
