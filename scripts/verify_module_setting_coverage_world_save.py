"""Closed world-operation contracts, separate from player administration.

Native contracts follow commands_world_save.rs; ordinary save commands follow
the declared runtime actions. Only games without a usable native command prepare a save extension at startup.
"""
from __future__ import annotations

NATIVE_WORLD_SAVE_CONTRACTS = {
    "satisfactory": ("satisfactory_https", "save"),
    "astroneer": ("astroneer_save", "save"),
    "valheim": ("valheim_native", "save"),
    "runescapedragonwilds": ("dragonwilds_save", "save"),
    "corekeeper": ("corekeeper_save", "save"),
    "vrising": ("vrising_save", "save"),
    "sonsoftheforest": ("sonsoftheforest_save", "save"),
    "abioticfactor": ("abioticfactor_save", "save"),
    "returntomoria": ("returntomoria_save", "save"),
    "windrose": ("windrose_save", "save"),
    "enshrouded": ("enshrouded_save", "save"),
    "rimworld": ("rimworld_save", "save"),
    "barotrauma": ("barotrauma_save", "save"),
    "soulmask": ("soulmask_save", "SaveWorld 1"),
    "nightingale": ("nightingale_save", "save"),
    "conanexiles": ("conanexiles_save", "save"),
    "scum": ("scum_save", "save_persistent_state"),
}
NATIVE_WORLD_SAVE_TRANSPORTS = frozenset(transport for transport, _ in NATIVE_WORLD_SAVE_CONTRACTS.values())
WORLD_OPERATION_IDS = frozenset({"save_world"})

# (transport, exact logical command, authoritative dispatch metadata).
WORLD_SAVE_CONTRACTS = {
    module: (transport, command, {})
    for module, (transport, command) in NATIVE_WORLD_SAVE_CONTRACTS.items()
}
WORLD_SAVE_CONTRACTS["astroneer"][2].update(port_name="console", password_setting_key="console_password")
for _module, _command in (
    ("minecraft", "save-all flush"), ("projectzomboid", "save"),
    ("terraria", "save"), ("unturned", "Save"), ("necesse", "save"),
    ("romestead", "save"), ("theforest", "save"),
):
    WORLD_SAVE_CONTRACTS[_module] = ("stdin", _command, {"process_key": "main"})
WORLD_SAVE_CONTRACTS["dontstarve"] = ("stdin", "c_save()", {"process_key": "master"})
for _ark in ("arksurvivalevolved", "arksurvivalascended"):
    WORLD_SAVE_CONTRACTS[_ark] = ("source_rcon", "saveworld", {
        "port_name": "rcon", "password_setting_key": "admin_password", "enabled_setting_key": "rcon_enabled"})
WORLD_SAVE_CONTRACTS["rust"] = ("websocket_rcon", "server.save", {
    "port_name": "rcon", "password_setting_key": "rcon_password", "enabled_setting_key": "rcon_web"})
WORLD_SAVE_CONTRACTS["palworld"] = ("palworld_rest", "save", {
    "port_name": "rest_api", "password_setting_key": "admin_password", "enabled_setting_key": "rest_api_enabled"})
WORLD_SAVE_CONTRACTS["humanitz"] = ("source_rcon", "save", {
    "port_name": "rcon", "password_setting_key": "rcon_password", "enabled_setting_key": "rcon_enabled"})
WORLD_SAVE_CONTRACTS["sevendaystodie"] = ("telnet", "saveworld", {
    "port_name": "telnet", "password_setting_key": "telnet_password", "enabled_setting_key": "telnet_enabled"})

_TARGET_FIELDS = (
    "target_required", "target_encoding", "target_label", "target_label_zh_cn",
    "target_placeholder", "target_placeholder_zh_cn", "role_values",
)
_DISPATCH_FIELDS = ("process_key", "port_name", "password_setting_key", "enabled_setting_key")


def is_world_operation(action: dict) -> bool:
    action_id, transport = action.get("id"), action.get("transport")
    return ((isinstance(action_id, str) and action_id in WORLD_OPERATION_IDS)
            or (isinstance(transport, str) and transport in NATIVE_WORLD_SAVE_TRANSPORTS))


def validate_world_operation(
    module_id: str, module: dict, action: dict,
    properties: dict[str, dict] | None = None,
    materialized_ports: set[str] | None = None,
) -> list[str]:
    action_id = action.get("id")
    name = f"{module_id}: runtime.player_actions[{action_id}]"
    contract = WORLD_SAVE_CONTRACTS.get(module_id)
    if action_id != "save_world":
        contract = None
    if contract is None or module.get("id") != module_id:
        return [f"{name} has no verified module-specific world-operation contract"]
    transport, command, metadata = contract
    failures = []
    if (action.get("transport") != transport or action.get("command_template") != command
            or action.get("kind") is not None
            or (action.get("destructive") is not None and action.get("destructive") is not False)
            or any(action.get(key) is not None for key in _TARGET_FIELDS)
            or any(action.get(key) != metadata.get(key) for key in _DISPATCH_FIELDS)):
        failures.append(f"{name} must use its exact targetless mutable world-operation contract")
    if properties is not None:
        port_name = metadata.get("port_name")
        if port_name is not None:
            ports = [port for port in module.get("default_ports", []) if port.get("name") == port_name]
            if len(ports) != 1 or str(ports[0].get("protocol", "")).lower() != "tcp":
                failures.append(f"{name} requires its declared TCP {port_name} port")
            if materialized_ports is not None and port_name not in materialized_ports:
                failures.append(f"{name} requires its {port_name} port to be materialized")
        for field in ("password_setting_key", "enabled_setting_key"):
            key = metadata.get(field)
            if key is not None and key not in properties:
                failures.append(f"{name} references unknown {field} {key}")
    return failures


def is_verified_world_operation(module: dict, action: dict) -> bool:
    return (is_world_operation(action)
            and not validate_world_operation(str(module.get("id") or ""), module, action))
