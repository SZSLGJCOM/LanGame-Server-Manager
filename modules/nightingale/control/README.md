# Native current-world save

Manual save invokes the game's existing periodic-save closure once on EngineTick. That closure derives its own periodic filename, calls `WorldPersistenceSubsystem::SaveWorldState` with its asynchronous flag enabled, and emits the normal periodic notification. The game checks persistence-enabled and required-load state, captures its world session and schedules its own task and storage callback. LanGame does not serialize a second snapshot, create a writer thread, select storage tokens or paths, or substitute a synchronous storage call.

The native entry returns no storage result. A returned call is `save_completion=requested`; it does not prove that native admission or persistence succeeded. Exceptions, mismatched identities, expired requests and missing acknowledgments remain unknown and are never resent. An in-flight request retains its instance mutation reservation and storage lease even when the caller is cancelled.

The call is an internal native ABI, not a discovered UFunction. It remains closed to Steam build **22204011**, Shipping SHA-256 `8e06c60524066655aee66da2e06b52c702a951afc3c28b59a2923f9dd51bd7eb`, and official UE4SS `3.0.1-1125-g527a483b`, core SHA-256 `91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958`.

| Fixed-image entry | Required contract |
| --- | --- |
| Periodic closure, RVA `0x3f8d190` | Receives the address of one captured `UWorldPersistenceSubsystem*`, not the UObject address itself. Calls `SaveWorldState(this, filename, true)`. |
| Class singleton, RVA `0x9f551e0` | Exact `WorldPersistenceSubsystem` class; its outer must be the current game instance. |
| Native weak-world resolver, RVA `0x1594190` | Resolves the subsystem's current world and must match the authoritative world selected by Lua. |

Lua selects the unique authoritative GameNetDriver and begun-play world, then verifies game state, game mode, owning game instance and subsystem. The native bridge separately checks the fixed image, loader, class, object lifetime and world ownership. The nonce, current boot/world, inherited managed-launch token, process ID and creation time bind both receipts to the current request. Native admission runs on the game thread; a saved unknown receipt precedes its single side effect. The existing bounded execution budget also covers native snapshot preparation without extending queue admission.

Stopped startup publishes the same owned `Scripts/main.lua`, `enabled.txt` and `native/LgsmNightingaleSaveControl.dll` with recoverable compare-and-swap installation. The earlier writer DLL is replaced through that existing record; no second bridge remains. Unknown or externally modified files are preserved. Loader and proxy ownership remain separate.

The fixed executable proves the periodic closure's argument shape, `SaveWorldState` guards and native task dispatch. The official server manual documents persistence paths and a read-only status endpoint; it does not expose a public manual-save endpoint. The Shipping console callbacks named `Persistence.SaveWorld` and `Persistence.SaveWorldAsync` are not used as save acknowledgments.

Component tests execute the real Lua policy and shared protocol, verify one capture-based native dispatch, and retain session/world/process, replay, deadline and malformed-frame rejection. They do not establish disk persistence. Revised native acceptance requires a pre-stop snapshot and independent restart readback; a `requested` receipt alone is insufficient.

References: [official dedicated-server manual](https://a.storyblok.com/f/239842/x/e8167d5c91/nightingale-dedicated-server-0-8-final.pdf), [fixed UE4SS revision](https://github.com/UE4SS-RE/RE-UE4SS/tree/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2).
