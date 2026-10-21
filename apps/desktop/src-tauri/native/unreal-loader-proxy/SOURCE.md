# Official UE4SS forwarding proxies

`scripts/build_unreal_loader_proxies.ps1` builds real `version.dll` and
`dsound.dll` forwarding proxies with the unmodified upstream generator at
[UE4SS commit 527a483b63b4dd0104fe1ca1a3934a06b87fcfb2](https://github.com/UE4SS-RE/RE-UE4SS/tree/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/proxy_generator).
It reuses the authoritative 32-file source manifest at
`modules/runescapedragonwilds/extensions/proxy-sources.json`, checks every
SHA-256 and Git blob identity, verifies the signed local Windows system target,
and generates the matching exports before compiling x64 Release. The build
retains the upstream UE4SS MIT and fmt licenses alongside its bundled outputs.
No task scratch path, renamed DWM proxy, runtime compiler, injector or Windows
system-directory modification is required by the application.

The six reviewed save adapters use `version.dll` for Abiotic Factor,
`dsound.dll` for Conan Exiles, and the fixed official archive's `dwmapi.dll` for
Return to Moria, Nightingale, SCUM and Windrose. These entry choices match the
ordinary-startup private native trials documented in each game's control
README. The UE4SS core remains the fixed official `3.0.1-1125-g527a483b` asset;
the runtime installer verifies the exact 8,718,500-byte archive SHA-256
`4f9762f812329a640c8cfa14444c2bb97ecc213b8320bd4c5433383c3eef48f7`
and selected runtime members independently. It installs only core, SDK backend,
licenses and the game-specific proxy; upstream example/debug mods are excluded.

`unreal_loader` requires the caller to retain stopped-instance and exclusive
private-program leases. It rejects reparse ancestors, validates the game image,
preserves unknown loader binaries, merges only required INI values and retains
operator fields/comments/mods. Publication uses a bounded ownership receipt,
compare-and-swap and a recoverable pending intent. The activation proxy is
published after the core and configuration. Startup and explicit preparation
share this installer; neither path writes into a shared program directory.

The configuration checks follow the fixed upstream
[SettingsManager](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/src/SettingsManager.cpp)
and [program startup](https://github.com/UE4SS-RE/RE-UE4SS/blob/527a483b63b4dd0104fe1ca1a3934a06b87fcfb2/UE4SS/src/UE4SSProgram.cpp).
Primary/additional/removal mod-directory overrides and game-specific config
directories can bypass or shadow the owned save bridge, so they are preserved
and reported as conflicts. Unreviewed engine-version/object-layout overrides are
also conflicts. Empty/default automatic detection remains compatible.
`ControllingModsTxt` is retained: the upstream loader independently starts mods
whose `enabled.txt` is present. Other operator configuration and mods remain
untouched by these targeted checks.
