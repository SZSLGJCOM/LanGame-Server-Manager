# Palworld native manual save

The `save_world` action uses the game's authenticated REST API. Its manifest
command `save` is rendered as the closed save operation and sent once as
`POST /v1/api/save`; it is not an arbitrary console command.

Enable `rest_api_enabled`, configure `admin_password` and a valid TCP `rest_api`
port, then restart after configuration changes. Saving does not install UE4SS,
the former native save observer or its bootstrap. Existing gameplay Mods,
loaders and operator configuration are retained.

A successful native submission returns `save_completion=requested`. Maintenance
and the server card report that the save request was sent and the game performs
the save. This is not a per-request disk-completion, content-readback or
power-loss guarantee. A delivery error or timeout remains unknown; the manager
does not automatically repeat the save. Check the game console before retrying.
