from __future__ import annotations

import json
import re
from pathlib import Path

from dst_world_options import verify_repository as verify_dst_world_options
from source_literal_scan import SourceLiteralState, source_literal_mask


MASTER_OVERRIDE_SECTIONS = {"mastergen", "mastersettings"}
CAVES_OVERRIDE_SECTIONS = {"cavesgen", "cavessettings"}
EXPECTED_MASTER_OVERRIDE_COUNT = 190
EXPECTED_CAVES_OVERRIDE_COUNT = 86
EXPECTED_CAVES_INHERITED_OVERRIDE_KEYS = {
    "basicresource_regrowth", "extrastartingitems", "seasonalstartingitems",
    "spawnprotection", "dropeverythingondespawn", "healthpenalty", "lessdamagetaken",
    "temperaturedamage", "hunger", "darkness", "shadowcreatures", "brightmarecreatures",
    "crow_carnival", "hallowed_nights", "winters_feast", "year_of_the_gobbler",
    "year_of_the_varg", "year_of_the_pig", "year_of_the_carrat", "year_of_the_beefalo",
    "year_of_the_catcoon", "year_of_the_bunnyman", "year_of_the_dragonfly",
    "year_of_the_snake", "year_of_the_knight", "specialevent", "day", "spawnmode",
    "ghostenabled", "portalresurection", "ghostsanitydrain", "resettime", "beefaloheat",
    "krampus",
}
LAUNCH_SETTING_FLAGS = {
    "disable_data_collection": "-disabledatacollection",
    "backup_log_count": "-backup_log_count",
    "backup_log_period": "-backup_log_period",
    "friends_only": "-fo",
    "allow_ioopenwrite_sandbox_escape": "-allow_ioopenwrite_sandbox_escape",
}
DST_NATIVE_SHARDS = ("master", "caves", "islands", "volcano")


def rust_function_body_from_source(source: str, function_name: str) -> str:
    declaration = re.search(rf"\bfn\s+{re.escape(function_name)}\b", source)
    if declaration is None:
        return ""
    brace_index = source.find("{", declaration.end())
    if brace_index < 0:
        return ""

    depth = 0
    in_string = False
    escaped = False
    for index in range(brace_index, len(source)):
        character = source[index]
        if in_string:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                in_string = False
            continue
        if character == '"':
            in_string = True
        elif character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return source[brace_index + 1 : index]
    return ""


def rust_const_tuple_string_values(source: str, const_name: str) -> list[tuple[str, str, str]]:
    match = re.search(
        rf"\bconst\s+{re.escape(const_name)}\s*:[^=]+?=\s*&\[(.*?)\];",
        source,
        re.DOTALL,
    )
    if not match:
        return []
    return [
        (setting_key, output_key, default_value)
        for setting_key, output_key, default_value in re.findall(
            r'\(\s*"([^"\\]+)"\s*,\s*"([^"\\]+)"\s*,\s*"([^"\\]+)"\s*,?\s*\)',
            match.group(1),
            re.DOTALL,
        )
    ]


def dst_override_inventory_settings(source: str, shard: str) -> set[str]:
    const_names = (
        ("DST_SHARED_OVERRIDE_ENTRIES", "DST_MASTER_OVERRIDE_ENTRIES")
        if shard == "master"
        else ("DST_CAVES_OVERRIDE_ENTRIES",)
    )
    return {
        setting_key
        for const_name in const_names
        for setting_key, _, _ in rust_const_tuple_string_values(source, const_name)
    }


def _is_executable_match(source: str, match: re.Match[str], *groups: str) -> bool:
    executable = rust_executable_source(source)
    return all(
        executable[match.start(group):match.end(group)] == match[group]
        for group in groups
    )


def _dst_token_arm(lookup: str, token: str) -> str | None:
    match = re.search(
        rf'"{re.escape(token)}"\s*(?P<arrow>=>)(?P<body>.*?)(?=^\s*(?:"[^"\n]+"\s*=>|_\s*=>))',
        lookup,
        re.DOTALL | re.MULTILINE,
    )
    return match["body"] if match and _is_executable_match(lookup, match, "arrow") else None


def dst_aggregate_token_settings(
    token: str, source: str, schema_properties: set[str]
) -> set[str] | None:
    """Resolve coverage only through actual native token routes and settings reads."""
    lookup = rust_function_body_from_source(source, "lookup_dst_template_token_with_instance")
    if token in {"shard_enabled", "caves_shard_id"}:
        arm = _dst_token_arm(lookup, token)
        if arm is None:
            return None
        reads = re.finditer(
            r'(?P<prefix>\bsettings\s*\.\s*get\s*\(\s*)"(?P<field>[a-z][a-z0-9_]*)"(?P<suffix>\s*\))',
            arm,
        )
        return {
            read["field"] for read in reads
            if _is_executable_match(arm, read, "prefix", "suffix")
        } & schema_properties

    suffix = next(
        (kind for kind in ("worldgenoverride", "modoverrides") if token.endswith(f"_{kind}")),
        None,
    )
    if suffix is None:
        return None
    shard = token.removesuffix(f"_{suffix}")
    if shard not in DST_NATIVE_SHARDS:
        return None
    renderer = f"render_dst_{suffix}"
    route = re.search(
        rf'"{re.escape(token)}"\s*(?P<prefix>=>\s*Some\(\s*{renderer}\(\s*settings\s*,\s*)'
        rf'"{shard}"(?P<suffix>\s*\)\s*\)\s*,)',
        lookup,
    )
    if not route or not _is_executable_match(lookup, route, "prefix", "suffix"):
        return None
    body = rust_function_body_from_source(source, renderer)
    coverage: set[str] = set()
    consumers = {
        "modoverrides_lua": "lookup_setting_text",
        "enabled_workshop_mod_ids": "parse_workshop_id_list",
        "mod_configuration_options": "render_dst_mod_configuration_block",
    } if suffix == "modoverrides" else {"worldgenoverride_lua": "lookup_setting_text"}
    for field_suffix, consumer in consumers.items():
        if suffix == "worldgenoverride" and shard not in {"islands", "volcano"}:
            continue
        assignment = re.search(
            rf'(?P<prefix>\blet\s+(?P<variable>[a-z][a-z0-9_]*)\s*=\s*format!\(\s*)'
            rf'"\{{shard\}}_{field_suffix}"(?P<suffix>\s*\)\s*;)',
            body,
        )
        if assignment and _is_executable_match(body, assignment, "prefix", "suffix"):
            variable = re.escape(assignment["variable"])
            if re.search(
                rf'\b{consumer}\s*\(\s*settings\s*,\s*&{variable}\s*(?:,|\))',
                rust_executable_source(body),
            ):
                coverage.add(f"{shard}_{field_suffix}")
    if suffix == "worldgenoverride" and shard in {"master", "caves"}:
        coverage.update(dst_override_inventory_settings(source, shard))
        prefixes = ("world_", "master_") if shard == "master" else ("caves_",)
        for read in re.finditer(
            r'(?P<prefix>\blookup_setting_text\s*\(\s*settings\s*,\s*)'
            r'"(?P<field>[a-z][a-z0-9_]*)"(?P<suffix>\s*\))', body,
        ):
            if _is_executable_match(body, read, "prefix", "suffix") and read["field"].startswith(prefixes):
                coverage.add(read["field"])
        # Both native preset branches bind three keys; credit each one only
        # when the corresponding bound variable is actually read from settings.
        selection = re.search(
            r'(?P<prefix>\blet\s*\((?P<variables>[^()]*)\)\s*=\s*if\s+shard\s*==\s*)'
            r'"master"(?P<open>\s*\{\s*\()(?P<master>.*?)\)\s*\}\s*else\s*\{\s*\('
            r'(?P<caves>.*?)\)\s*\}\s*;', body, re.DOTALL,
        )
        if selection and _is_executable_match(body, selection, "prefix", "open"):
            variables = [name.strip() for name in selection["variables"].split(",") if name.strip()]
            keys = re.findall(r'"([a-z][a-z0-9_]*)"', selection[shard])
            if len(variables) == len(keys):
                executable = rust_executable_source(body)
                for variable, key in zip(variables, keys):
                    if re.search(rf'\blookup_setting_text\s*\(\s*settings\s*,\s*{re.escape(variable)}\s*\)', executable):
                        coverage.add(key)
    return coverage & schema_properties


def _format_inventory_delta(expected: set[str], actual: set[str]) -> str:
    return f"missing={sorted(expected - actual)}, extra={sorted(actual - expected)}"


def validate_dst_override_inventory(
    schema_property_defs: dict[str, dict], storage_templates_source: str
) -> list[str]:
    failures: list[str] = []
    entries = {
        const_name: rust_const_tuple_string_values(storage_templates_source, const_name)
        for const_name in (
            "DST_SHARED_OVERRIDE_ENTRIES",
            "DST_MASTER_OVERRIDE_ENTRIES",
            "DST_CAVES_OVERRIDE_ENTRIES",
        )
    }
    for const_name, const_entries in entries.items():
        if not const_entries:
            failures.append(f"dontstarve: missing or empty native override inventory {const_name}")
            continue
        keys = [setting_key for setting_key, _, _ in const_entries]
        duplicates = sorted({key for key in keys if keys.count(key) > 1})
        if duplicates:
            failures.append(
                f"dontstarve: native override inventory {const_name} duplicates {duplicates}"
            )

    expected_master = {
        key
        for key, definition in schema_property_defs.items()
        if definition.get("x-lsgm-section") in MASTER_OVERRIDE_SECTIONS
        and str(definition.get("x-lsgm-source-key", "")).startswith("overrides.")
    }
    expected_caves = {
        key
        for key, definition in schema_property_defs.items()
        if definition.get("x-lsgm-section") in CAVES_OVERRIDE_SECTIONS
        and str(definition.get("x-lsgm-source-key", "")).startswith("overrides.")
    }
    actual_master = dst_override_inventory_settings(storage_templates_source, "master")
    actual_caves = dst_override_inventory_settings(storage_templates_source, "caves")

    for label, expected, actual, exact_count, rendered_entries in (
        (
            "Master",
            expected_master,
            actual_master,
            EXPECTED_MASTER_OVERRIDE_COUNT,
            entries["DST_SHARED_OVERRIDE_ENTRIES"] + entries["DST_MASTER_OVERRIDE_ENTRIES"],
        ),
        (
            "Caves",
            expected_caves,
            actual_caves,
            EXPECTED_CAVES_OVERRIDE_COUNT,
            entries["DST_CAVES_OVERRIDE_ENTRIES"],
        ),
    ):
        if len(expected) != exact_count:
            failures.append(
                f"dontstarve: schema {label} native inventory must contain exactly {exact_count} fields, found {len(expected)}"
            )
        if len(actual) != exact_count or actual != expected:
            failures.append(
                f"dontstarve: renderer {label} native inventory must exactly match {exact_count} schema fields; "
                + _format_inventory_delta(expected, actual)
            )
        output_keys = [output_key for _, output_key, _ in rendered_entries]
        duplicate_outputs = sorted({key for key in output_keys if output_keys.count(key) > 1})
        if duplicate_outputs:
            failures.append(
                f"dontstarve: renderer {label} native inventory duplicates output keys {duplicate_outputs}"
            )
        for setting_key, output_key, rendered_default in rendered_entries:
            definition = schema_property_defs.get(setting_key, {})
            schema_source_key = str(definition.get("x-lsgm-source-key", ""))
            if schema_source_key.removeprefix("overrides.") != output_key:
                failures.append(
                    f"dontstarve: renderer {label} field {setting_key} writes {output_key!r}, expected {schema_source_key!r}"
                )
            if definition.get("default") != rendered_default:
                failures.append(
                    f"dontstarve: renderer {label} field {setting_key} default {rendered_default!r} does not match schema {definition.get('default')!r}"
                )

    renderer_body = rust_function_body_from_source(
        storage_templates_source, "render_dst_worldgenoverride"
    )
    inherited_match = re.search(
        r"\bconst\s+DST_CAVES_INHERITED_OVERRIDE_KEYS\s*:[^=]+?=\s*&\[(.*?)\];",
        storage_templates_source,
        re.DOTALL,
    )
    inherited_keys = re.findall(r'"([^"\\]+)"', inherited_match.group(1)) if inherited_match else []
    if (
        set(inherited_keys) != EXPECTED_CAVES_INHERITED_OVERRIDE_KEYS
        or len(inherited_keys) != len(EXPECTED_CAVES_INHERITED_OVERRIDE_KEYS)
    ):
        failures.append(
            "dontstarve: Caves inherited inventory must match the 34 native masteroption keys; "
            + _format_inventory_delta(EXPECTED_CAVES_INHERITED_OVERRIDE_KEYS, set(inherited_keys))
        )
    inherited_body = rust_function_body_from_source(
        storage_templates_source, "dst_caves_inherited_override_entries"
    )
    for marker in ("DST_SHARED_OVERRIDE_ENTRIES", "DST_MASTER_OVERRIDE_ENTRIES", "DST_CAVES_INHERITED_OVERRIDE_KEYS.contains"):
        if marker not in inherited_body:
            failures.append(f"dontstarve: Caves inherited resolver is missing marker {marker!r}")
    for marker in (
        "DST_SHARED_OVERRIDE_ENTRIES",
        "dst_shard_override_entries(shard)",
        "push_dst_worldgen_entry",
        "dst_caves_inherited_override_entries()",
        "inherited_master_settings(settings)",
    ):
        if marker not in renderer_body:
            failures.append(
                f"dontstarve: worldgen renderer no longer consumes native inventory marker {marker!r}"
            )
    return failures


def rust_executable_source(source: str) -> str:
    state = SourceLiteralState()
    code = []
    for line in source.splitlines(keepends=True):
        mask = source_literal_mask(Path("source.rs"), line, state)
        code.append("".join(" " if hidden else character for character, hidden in zip(line, mask)))
    return "".join(code)


def validate_dst_private_setup_contract(source: str) -> list[str]:
    failures: list[str] = []
    dispatcher = rust_function_body_from_source(source, "materialize_module_support_files_pending")
    dispatcher_code = rust_executable_source(dispatcher)
    routes = re.finditer(
        r'"dontstarve"\s*(?P<route>=>\s*sync_dst_mod_setup\s*\(\s*context\s*,\s*files\s*,?\s*\)\s*\?\s*,)',
        dispatcher,
    )
    if not any(
        re.sub(r"\s+", "", dispatcher_code[route.start("route"):route.end("route")])
        in {"=>sync_dst_mod_setup(context,files)?,", "=>sync_dst_mod_setup(context,files,)?,"}
        for route in routes
    ):
        failures.append("dontstarve: support dispatcher must propagate private setup failures")
    source = rust_executable_source(source)
    body = re.sub(r"\s+", "", rust_function_body_from_source(source, "sync_dst_mod_setup"))
    root = re.search(r"let(\w+)=context\.config_dir\.parent\(\)\.unwrap_or\(context\.config_dir\);", body)
    resolved = re.search(
        rf"let(\w+)=crate::program_runtime::resolve_instance_runtime_root\({re.escape(root[1])},?\)\?;",
        body[root.end():],
    ) if root else None
    guard = re.search(
        rf"if{re.escape(resolved[1])}!=context\.install_root\{{"
        r"returnErr\(StorageError::UnsafeManagedPath\{[^{}]*\}\);\}",
        body[root.end() + resolved.end():],
    ) if root and resolved else None
    independent = re.search(
        rf"ifcrate::program_runtime::instance_program_mode\({re.escape(root[1])},?\)\?"
        r"!=crate::InstanceProgramMode::Independent\{"
        r"returnErr\(crate::program_runtime::invalid\([^{}]*\)\);\}",
        body[root.end():],
    ) if root else None
    calls = list(re.finditer(r"write_dst_mod_setup\(([^()]*)\)", body))
    if not (root and independent and resolved and guard and calls and
            independent.end() <= resolved.start() and
            root.end() + resolved.end() + guard.end() <= calls[0].start()):
        failures.append("dontstarve: setup must validate the instance private runtime before writing")
    if len(calls) != 1 or calls[0][1].rstrip(",") != "context.install_root,context.settings,files":
        failures.append("dontstarve: setup must use only this instance's settings and file transaction")

    writer = re.sub(r"\s+", "", rust_function_body_from_source(source, "write_dst_mod_setup"))
    rendered = re.search(r"let(\w+)=render_dst_mod_setup\(settings\);", writer)
    if not rendered or not re.search(
        rf"files\.write\(&\w+,{re.escape(rendered[1])}\.as_bytes\(\),?\)$", writer
    ) or "fs::write(" in writer:
        failures.append("dontstarve: setup renderer output must be written through the supplied file transaction")
    return failures


def validate_dst_instance_config_commit_contract(
    storage_instances_source: str, storage_templates_source: str
) -> list[str]:
    failures: list[str] = []
    private_setup_failures = validate_dst_private_setup_contract(storage_templates_source)
    storage_instances_source = rust_executable_source(storage_instances_source)
    storage_templates_source = rust_executable_source(storage_templates_source)
    creation_wrapper = rust_function_body_from_source(
        storage_instances_source, "create_instance_transaction"
    )
    if not re.search(
        r"\bcreate_instance_in_pool\(paths,descriptor,input,options,module_creation_lock,&pool,?\)\.await\b",
        re.sub(r"\s+", "", creation_wrapper),
    ):
        failures.append(
            "dontstarve: create_instance_transaction must await create_instance_in_pool with its transaction inputs"
        )
    lifecycle_materializers = {
        "create_instance_in_pool": ("materialize_new_instance_files(", "module_creation_lock"),
        "update_instance_transaction": ("write_pending_instance_configuration_in_worker(", "settings_lock"),
        "materialize_instance_configuration_transaction": ("write_pending_instance_configuration_in_worker(", "settings_lock"),
    }
    for function_name, (materializer_marker, lock_name) in lifecycle_materializers.items():
        body = rust_function_body_from_source(storage_instances_source, function_name)
        if not body:
            failures.append(f"dontstarve: missing instance lifecycle function {function_name}")
            continue
        helper_index = body.find(materializer_marker)
        commit_index = body.find(f"commit_instance_transaction(tx, config_mutation, {lock_name}).await?;")
        if helper_index < 0 or commit_index < 0 or helper_index >= commit_index:
            failures.append(
                f"dontstarve: {function_name} must coordinate support files and instance.json before committing its transaction"
            )
        materializer_body = (
            rust_function_body_from_source(storage_instances_source, "materialize_new_instance_files")
            if function_name == "create_instance_in_pool"
            else rust_function_body_from_source(
                storage_templates_source, "write_pending_instance_configuration_in_worker"
            )
        )
        unified_helper = (
            "write_pending_instance_configuration("
            if function_name == "create_instance_in_pool"
            else "write_pending_configuration_with_native_snapshot("
        )
        without_unified_call = materializer_body.replace(unified_helper, "", 1)
        split_write_bodies = [without_unified_call, body.replace(materializer_marker, "", 1)]
        if function_name == "create_instance_in_pool":
            split_write_bodies.append(creation_wrapper)
        if (
            unified_helper not in materializer_body
            or any(
                re.search(
                    r"(?<![A-Za-z0-9_])(?:materialize_module_support_files|write_instance_config)\s*\(",
                    candidate_body,
                )
                for candidate_body in split_write_bodies
            )
        ):
            failures.append(
                f"dontstarve: {function_name} must not split support-file and instance.json writes"
            )

    for function_name, marker in (
        (
            "materialize_instance_configuration_for_start",
            "materialize_instance_configuration_with_policy(paths, instance_id, &settings_lock, true).await",
        ),
        (
            "materialize_instance_configuration_locked",
            "materialize_instance_configuration_with_policy(paths, instance_id, settings_lock, false).await",
        ),
    ):
        if marker not in rust_function_body_from_source(storage_instances_source, function_name):
            failures.append(
                f"dontstarve: {function_name} no longer routes through the coordinated materialization policy"
            )

    for wrapper, transaction in (
        ("create_instance_with_options", "create_instance_transaction"),
        ("update_instance_with_baseline_locked", "update_instance_transaction"),
        ("materialize_instance_configuration_with_policy", "materialize_instance_configuration_transaction"),
        ("delete_instance", "delete_instance_locked"),
    ):
        body = rust_function_body_from_source(storage_instances_source, wrapper)
        if ".complete_mutation(" not in body or f"{transaction}(" not in body:
            failures.append(
                f"dontstarve: {wrapper} must retain mutation ownership until its transaction finishes"
            )

    for source, function, call in (
        (storage_instances_source, "update_instance_locked",
         "update_instance_with_baseline_locked(paths,input,settings_lock,None).await"),
        (storage_templates_source, "write_pending_instance_configuration",
         "write_pending_configuration_with_native_snapshot(templates_root,render_input,context,config_path,input,prepared,None,)"),
    ):
        if call not in re.sub(r"\s+", "", rust_function_body_from_source(source, function)):
            failures.append(f"dontstarve: {function} must delegate to the coordinated native configuration path")

    helper_body = rust_function_body_from_source(storage_templates_source, "write_pending_configuration_with_native_snapshot")
    for marker in (
        "render_module_templates_with_writer(",
        "write_rendered_preserving_ark_ini(render_input, path, rendered, &mut files)",
        "materialize_module_support_files_pending(context, &mut files, prepared)?;",
        "files.write(config_path, &render_instance_config(input)?)?;",
        "Err(error) => Err(files.rollback_after(error))",
    ):
        if marker not in helper_body:
            failures.append(
                f"dontstarve: unified support/config helper is missing marker {marker!r}"
            )

    writer_body = rust_function_body_from_source(storage_templates_source, "write_rendered")
    if "input.module_id" not in writer_body or "return files.write(path, rendered.as_bytes())" not in writer_body:
        failures.append("dontstarve: ARK-aware writer must keep other modules on the coordinated file transaction")

    support_call = "materialize_module_support_files_pending(context,&mutfiles,prepared)?;"
    helper_code = re.sub(r"\s+", "", helper_body)
    if helper_code.count(support_call) != 1 or (
        "if!context.instance_running{" + support_call + "}"
    ) not in helper_code:
        failures.append("dontstarve: support files must stay in the stopped-instance file transaction")
    failures.extend(private_setup_failures)

    worker_body = rust_function_body_from_source(storage_templates_source, "write_pending_instance_configuration_in_worker")
    if not re.search(r"settings_lock\s*\.spawn_blocking\s*\(", worker_body):
        failures.append("dontstarve: configuration worker must retain the instance mutation lease while blocking")

    commit_body = rust_function_body_from_source(
        storage_instances_source, "commit_instance_transaction"
    )
    missing = [marker for marker in ("config_mutation.commit();", "config_mutation.rollback_after(original)") if marker not in commit_body]
    failures.extend(f"dontstarve: transaction compensation no longer preserves marker {marker!r}" for marker in missing)
    commit_code = re.sub(r"\s+", "", commit_body)
    commit = re.search(
        r"matchtx\.commit\(\)\.await\{Ok\(\(\)\)=>\{config_mutation\.commit\(\);Ok\(\(\)\)\},?"
        r"Err\((?P<error>\w+)\)=>\{(?P<failure>.*)\},?\}$", commit_code,
    )
    if not missing and (not commit or not all(marker in commit["failure"] for marker in (
        f"letoriginal=StorageError::Sqlx({commit['error']});",
        "settings_lock.spawn_blocking(move||config_mutation.rollback_after(original)).await",
        "Ok(error)=>Err(error)",
    ))):
        failures.append("dontstarve: database commit must finalize files on success and compensate on failure")
    return failures


def validate_dst_token_resolvers(
    storage_templates_source: str,
    runtime_launch_source: str,
    module_manifest_source: str,
    caves_server_template_source: str,
) -> list[str]:
    failures: list[str] = []
    storage_lookup = rust_function_body_from_source(
        storage_templates_source, "lookup_dst_template_token_with_instance"
    )
    storage_mappings = {
        "admin_list_lines": 'render_dst_klei_user_id_lines(settings, "admin_list")',
        "whitelist_lines": 'render_dst_klei_user_id_lines(settings, "whitelist")',
        "blocklist_lines": 'render_dst_klei_user_id_lines(settings, "blocklist")',
        "cluster_intention_line": "render_dst_cluster_intention_line(settings)",
        "caves_shard_id": "derive_dst_caves_shard_id(instance_id).to_string()",
    }
    storage_mappings.update({
        f"{shard}_{kind}": f'render_dst_{kind}(settings, "{shard}")'
        for shard in DST_NATIVE_SHARDS for kind in ("worldgenoverride", "modoverrides")
    })
    for token, resolver in storage_mappings.items():
        if f'"{token}"' not in storage_lookup or resolver not in storage_lookup:
            failures.append(
                f"dontstarve: storage template token dst.{token} is missing its native resolver"
            )
    for token, expected in (
        ("shard_enabled", {"shard_layout", "enable_caves"}),
        ("caves_shard_id", {"shard_layout"}),
    ):
        if dst_aggregate_token_settings(token, storage_templates_source, expected) != expected:
            failures.append(
                f"dontstarve: dst.{token} must consume its effective shard layout settings"
            )

    shard_id_body = rust_function_body_from_source(
        storage_templates_source, "derive_dst_caves_shard_id"
    )
    for marker in ("instance_id.as_bytes()", ".max(2)"):
        if marker not in shard_id_body:
            failures.append(
                f"dontstarve: stable Caves shard ID resolver is missing marker {marker!r}"
            )

    launch_lookup = rust_function_body_from_source(
        runtime_launch_source, "lookup_dontstarve_launch_token"
    )
    launch_renderer = rust_function_body_from_source(
        runtime_launch_source, "render_dontstarve_launch_args"
    )
    if (
        '"launch_args"' not in launch_lookup
        or "render_dontstarve_launch_args(context.settings).join(\"\\n\")" not in launch_lookup
    ):
        failures.append(
            "dontstarve: runtime token dontstarve.launch_args is missing its argv resolver"
        )
    for setting_key, flag in LAUNCH_SETTING_FLAGS.items():
        if f'"{setting_key}"' not in launch_renderer or f'"{flag}"' not in launch_renderer:
            failures.append(
                f"dontstarve: launch argv renderer no longer maps {setting_key} to {flag}"
            )

    if "{{dontstarve.launch_args}}" not in module_manifest_source:
        failures.append(
            "dontstarve: process args_template must include the native dontstarve.launch_args token"
        )
    if "{{dst.caves_shard_id}}" not in caves_server_template_source:
        failures.append(
            "dontstarve: Caves server.ini must derive its stable id from dst.caves_shard_id"
        )
    return failures


def validate_dst_native_settings_contract(
    root: Path,
    *,
    schema_property_defs: dict[str, dict] | None = None,
    storage_templates_source: str | None = None,
    storage_instances_source: str | None = None,
    storage_deletion_source: str | None = None,
    runtime_launch_source: str | None = None,
    module_manifest_source: str | None = None,
    caves_server_template_source: str | None = None,
) -> list[str]:
    module_root = root / "modules" / "dontstarve"
    storage_root = root / "crates" / "app-storage" / "src"
    if schema_property_defs is None:
        schema = json.loads((module_root / "schema.json").read_text(encoding="utf-8-sig"))
        schema_property_defs = schema.get("properties", {})
    if storage_templates_source is None:
        paths = sorted(storage_root.glob("templates*.rs"))
        paths.extend(sorted((storage_root / "templates_materialize").glob("*.rs")))
        storage_templates_source = "\n".join(
            path.read_text(encoding="utf-8-sig") for path in paths if path.is_file()
        )
    if storage_instances_source is None:
        storage_instances_source = (storage_root / "instances.rs").read_text(encoding="utf-8-sig")
    if storage_deletion_source is None:
        storage_deletion_source = (storage_root / "instance_deletion.rs").read_text(encoding="utf-8-sig")
    if runtime_launch_source is None:
        runtime_launch_source = (
            root / "crates" / "app-runtime" / "src" / "launch_templates.rs"
        ).read_text(encoding="utf-8-sig")
    if module_manifest_source is None:
        module_manifest_source = (module_root / "module.toml").read_text(encoding="utf-8-sig")
    if caves_server_template_source is None:
        caves_server_template_source = (
            module_root / "templates" / "clusters" / "main" / "Caves" / "server.ini.hbs"
        ).read_text(encoding="utf-8-sig")

    return [
        *verify_dst_world_options(root),
        *validate_dst_override_inventory(schema_property_defs, storage_templates_source),
        *validate_dst_instance_config_commit_contract(
            storage_instances_source + "\n" + storage_deletion_source, storage_templates_source
        ),
        *validate_dst_token_resolvers(
            storage_templates_source,
            runtime_launch_source,
            module_manifest_source,
            caves_server_template_source,
        ),
    ]
