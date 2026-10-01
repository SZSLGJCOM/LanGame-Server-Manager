# HumanitZ info response

These fixtures are synthetic protocol examples, not captures from a running server or real player data.

The maintained HumanitZ client documents the `info` response header, the `N connected.` count, and the following `Players:` section with one display name per line:

- <https://github.com/Minidoracat/humanitz-bot/blob/main/src/humanitz_bot/services/rcon_service.py>
- <https://github.com/Minidoracat/humanitz-bot/blob/main/src/humanitz_bot/rcon_client.py>

The client documents fixed response ID 0, authentication type 0 followed by type 2, and no response to an empty command. LanGame uses a dedicated `humanitz_rcon` transport and only enables the read-only `info` query. It requires the explicit count, all documented header fields, the Players section, and exactly that many complete newline-terminated name lines. A missing final newline remains incomplete; a packet boundary or idle timeout does not prove that a name finished. Zero requires the full header, explicit zero, and an empty Players section. Unknown, malformed, short, oversized, or timed-out replies are errors.

`info` supplies display names without account IDs. Duplicate names remain separate read-only rows; they are never used as kick or ban targets. The unrelated `Players` command's lack of a documented completion and empty-list boundary prevents using it to populate actionable identities.

The installed dedicated package examined during implementation was Steam build 23914958. Its native RCON sender enqueues response data, but the encrypted packaged command implementation does not establish a stronger response completion boundary. No player saves or roster data were inspected.
