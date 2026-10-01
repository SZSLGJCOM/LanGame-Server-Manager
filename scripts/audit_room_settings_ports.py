from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
PORT_REF_RE = re.compile(r"\{\{\s*ports\.([A-Za-z0-9_-]+)\.port\s*\}\}")
VALID_PROTOCOLS = {"tcp", "udp"}


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig")


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(read_text(path))


def write_json(path: Path, data: dict[str, Any]) -> None:
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def normalize_protocol(value: Any) -> str:
    return str(value or "").strip().lower()


def normalize_port_binding(raw: Any) -> dict[str, Any] | None:
    if not isinstance(raw, dict):
        return None
    name = str(raw.get("name") or "").strip()
    protocol = normalize_protocol(raw.get("protocol"))
    try:
        port = int(raw.get("port"))
    except (TypeError, ValueError):
        return None
    if not name or protocol not in VALID_PROTOCOLS or port < 0 or port > 65535:
        return None
    return {"name": name, "protocol": protocol, "port": port}


def port_identity(port: dict[str, Any]) -> tuple[str, str]:
    return str(port["name"]), str(port["protocol"])


def listener_identity(port: dict[str, Any]) -> tuple[str, int]:
    return str(port["protocol"]), int(port["port"])


def relative(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def module_dirs() -> list[Path]:
    return sorted(
        path
        for path in MODULES_DIR.iterdir()
        if path.is_dir() and (path / "module.toml").exists()
    )


def collect_template_port_refs(module_dir: Path) -> list[dict[str, str]]:
    refs: list[dict[str, str]] = []
    files = [module_dir / "module.toml"]
    templates_dir = module_dir / "templates"
    if templates_dir.exists():
        files.extend(path for path in sorted(templates_dir.rglob("*")) if path.is_file())

    for path in files:
        text = read_text(path)
        for match in PORT_REF_RE.finditer(text):
            refs.append({"name": match.group(1), "source": relative(path)})
    return refs


def collect_structured_port_refs(value: Any, source: str, refs: list[dict[str, str]]) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key == "port_name" and isinstance(child, str) and child.strip():
                refs.append({"name": child.strip(), "source": source})
            collect_structured_port_refs(child, source, refs)
        return
    if isinstance(value, list):
        for child in value:
            collect_structured_port_refs(child, source, refs)


def collect_modules() -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    modules: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []

    for module_dir in module_dirs():
        module_id = module_dir.name
        module_toml_path = module_dir / "module.toml"
        data = tomllib.loads(read_text(module_toml_path))
        default_ports_raw = data.get("default_ports") or []
        default_ports: list[dict[str, Any]] = []

        for index, raw in enumerate(default_ports_raw):
            normalized = normalize_port_binding(raw)
            if normalized is None:
                failures.append(
                    {
                        "scope": "module",
                        "type": "invalid_default_port",
                        "module_id": module_id,
                        "index": index,
                        "source": relative(module_toml_path),
                    }
                )
                continue
            default_ports.append(normalized)

        declared = {port_identity(port) for port in default_ports}
        declared_names = {name for name, _protocol in declared}

        seen_bindings: dict[tuple[str, str], dict[str, Any]] = {}
        seen_listeners: dict[tuple[str, int], dict[str, Any]] = {}
        for port in default_ports:
            binding_key = port_identity(port)
            listener_key = listener_identity(port)
            if binding_key in seen_bindings:
                failures.append(
                    {
                        "scope": "module",
                        "type": "duplicate_default_port_binding",
                        "module_id": module_id,
                        "port": port,
                        "source": relative(module_toml_path),
                    }
                )
            if listener_key in seen_listeners:
                failures.append(
                    {
                        "scope": "module",
                        "type": "duplicate_default_listener",
                        "module_id": module_id,
                        "port": port,
                        "source": relative(module_toml_path),
                    }
                )
            seen_bindings[binding_key] = port
            seen_listeners[listener_key] = port

        refs = collect_template_port_refs(module_dir)
        collect_structured_port_refs(data, relative(module_toml_path), refs)
        referenced_names = sorted({ref["name"] for ref in refs})
        missing_refs = [name for name in referenced_names if name not in declared_names]
        for name in missing_refs:
            failures.append(
                {
                    "scope": "module",
                    "type": "missing_declared_port",
                    "module_id": module_id,
                    "port_name": name,
                    "sources": sorted({ref["source"] for ref in refs if ref["name"] == name}),
                }
            )

        modules.append(
            {
                "module_id": module_id,
                "name": str(data.get("name") or module_id),
                "default_port_count": len(default_ports),
                "default_ports": default_ports,
                "port_ref_count": len(referenced_names),
                "port_refs": referenced_names,
                "missing_port_refs": missing_refs,
            }
        )

    return modules, failures


def instance_config_paths(instances_root: Path, exclude_hidden: bool) -> list[Path]:
    if not instances_root.exists():
        return []

    paths: list[Path] = []
    for path in sorted(instances_root.rglob("config/instance.json")):
        if exclude_hidden:
            try:
                parts = path.relative_to(instances_root).parts
            except ValueError:
                parts = path.parts
            if any(part.startswith(".") for part in parts):
                continue
        paths.append(path)
    return paths


def path_has_hidden_part(path: Path, root: Path) -> bool:
    try:
        parts = path.relative_to(root).parts
    except ValueError:
        parts = path.parts
    return any(part.startswith(".") for part in parts)


def collect_instances(
    instances_root: Path,
    modules_by_id: dict[str, dict[str, Any]],
    exclude_hidden: bool,
    repair_missing: bool,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    instances: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []
    repairs: list[dict[str, Any]] = []

    for config_path in instance_config_paths(instances_root, exclude_hidden):
        try:
            data = read_json(config_path)
        except (OSError, json.JSONDecodeError) as error:
            failures.append(
                {
                    "scope": "instance",
                    "type": "invalid_instance_config",
                    "path": config_path.as_posix(),
                    "message": str(error),
                }
            )
            continue

        instance_id = str(data.get("instance_id") or config_path.parent.parent.name)
        module_id = str(data.get("module_id") or "")
        raw_ports = data.get("ports") or []
        ports: list[dict[str, Any]] = []
        instance_failures: list[dict[str, Any]] = []

        for index, raw in enumerate(raw_ports):
            normalized = normalize_port_binding(raw)
            if normalized is None:
                failure = {
                    "scope": "instance",
                    "type": "invalid_instance_port",
                    "instance_id": instance_id,
                    "module_id": module_id,
                    "index": index,
                    "path": config_path.as_posix(),
                }
                failures.append(failure)
                instance_failures.append(failure)
                continue
            ports.append(normalized)

        seen_bindings: dict[tuple[str, str], dict[str, Any]] = {}
        seen_listeners: dict[tuple[str, int], dict[str, Any]] = {}
        for port in ports:
            binding_key = port_identity(port)
            listener_key = listener_identity(port)
            if binding_key in seen_bindings:
                failure = {
                    "scope": "instance",
                    "type": "duplicate_instance_port_binding",
                    "instance_id": instance_id,
                    "module_id": module_id,
                    "port": port,
                    "path": config_path.as_posix(),
                }
                failures.append(failure)
                instance_failures.append(failure)
            if listener_key in seen_listeners:
                failure = {
                    "scope": "instance",
                    "type": "duplicate_instance_listener",
                    "instance_id": instance_id,
                    "module_id": module_id,
                    "port": port,
                    "path": config_path.as_posix(),
                }
                failures.append(failure)
                instance_failures.append(failure)
            seen_bindings[binding_key] = port
            seen_listeners[listener_key] = port

        module = modules_by_id.get(module_id)
        missing_ports: list[dict[str, Any]] = []
        extra_ports: list[dict[str, Any]] = []
        if module is None:
            failure = {
                "scope": "instance",
                "type": "unknown_instance_module",
                "instance_id": instance_id,
                "module_id": module_id,
                "path": config_path.as_posix(),
            }
            failures.append(failure)
            instance_failures.append(failure)
        else:
            declared_by_identity = {
                port_identity(port): port
                for port in module["default_ports"]
            }
            actual_by_identity = {
                port_identity(port): port
                for port in ports
            }
            missing_ports = [
                declared_by_identity[key]
                for key in sorted(set(declared_by_identity) - set(actual_by_identity))
            ]
            extra_ports = [
                actual_by_identity[key]
                for key in sorted(set(actual_by_identity) - set(declared_by_identity))
            ]

            for port in missing_ports:
                failure = {
                    "scope": "instance",
                    "type": "missing_instance_port",
                    "instance_id": instance_id,
                    "module_id": module_id,
                    "port": port,
                    "path": config_path.as_posix(),
                }
                failures.append(failure)
                instance_failures.append(failure)

            if repair_missing and missing_ports:
                merged_ports = list(raw_ports) if isinstance(raw_ports, list) else []
                merged_ports.extend(missing_ports)
                data["ports"] = merged_ports
                write_json(config_path, data)
                repairs.append(
                    {
                        "instance_id": instance_id,
                        "module_id": module_id,
                        "path": config_path.as_posix(),
                        "added_ports": missing_ports,
                    }
                )

        instances.append(
            {
                "instance_id": instance_id,
                "module_id": module_id,
                "name": str(data.get("instance_name") or instance_id),
                "path": config_path.as_posix(),
                "hidden_path": path_has_hidden_part(config_path, instances_root),
                "port_count": len(ports),
                "ports": ports,
                "missing_ports": missing_ports,
                "extra_ports": extra_ports,
                "failure_count": len(instance_failures),
            }
        )

    return instances, failures, repairs


def build_report(
    instances_root: Path | None = None,
    exclude_hidden: bool = False,
    repair_missing: bool = False,
) -> dict[str, Any]:
    if repair_missing and instances_root is None:
        raise ValueError("--repair-missing requires --instances-root")
    modules, module_failures = collect_modules()
    modules_by_id = {module["module_id"]: module for module in modules}
    instances: list[dict[str, Any]] = []
    instance_failures: list[dict[str, Any]] = []
    repairs: list[dict[str, Any]] = []
    if instances_root is not None:
        instances, instance_failures, repairs = collect_instances(
            instances_root,
            modules_by_id,
            exclude_hidden,
            repair_missing,
        )
        if repair_missing:
            instances, instance_failures, _repairs_after = collect_instances(
                instances_root,
                modules_by_id,
                exclude_hidden,
                False,
            )
    failures = module_failures + instance_failures

    module_ids_with_instances = sorted({instance["module_id"] for instance in instances})
    modules_without_instances = [
        module["module_id"]
        for module in modules
        if module["module_id"] not in module_ids_with_instances
    ]

    return {
        "summary": {
            "module_count": len(modules),
            "module_port_count": sum(module["default_port_count"] for module in modules),
            "module_port_ref_count": sum(module["port_ref_count"] for module in modules),
            "instance_count": len(instances),
            "instance_port_count": sum(instance["port_count"] for instance in instances),
            "failure_count": len(failures),
            "repair_count": len(repairs),
            "modules_without_instances_count": len(modules_without_instances),
        },
        "instances_root": instances_root.as_posix() if instances_root is not None else None,
        "modules_without_instances": modules_without_instances,
        "modules": modules,
        "instances": instances,
        "repairs": repairs,
        "failures": failures,
    }


def format_port(port: dict[str, Any]) -> str:
    return f"{port['name']}/{port['protocol']}:{port['port']}"


def write_markdown(path: Path, report: dict[str, Any]) -> None:
    summary = report["summary"]
    lines = [
        "# Room Settings Port Audit",
        "",
        f"- modules: {summary['module_count']}",
        f"- declared module ports: {summary['module_port_count']}",
        f"- module port references: {summary['module_port_ref_count']}",
        f"- instances: {summary['instance_count']}",
        f"- registered instance ports: {summary['instance_port_count']}",
        f"- failures: {summary['failure_count']}",
        f"- repairs: {summary['repair_count']}",
        "",
    ]

    if report["failures"]:
        lines.extend(["## Failures", ""])
        for failure in report["failures"]:
            subject = failure.get("instance_id") or failure.get("module_id") or failure.get("path") or "unknown"
            port = failure.get("port")
            suffix = f" `{format_port(port)}`" if isinstance(port, dict) else ""
            lines.append(f"- `{failure['type']}` `{subject}`{suffix}")
        lines.append("")

    lines.extend(
        [
            "## Instances",
            "",
            "| instance | module | ports | missing | extra | path |",
            "| --- | --- | ---: | --- | --- | --- |",
        ]
    )
    for instance in report["instances"]:
        missing = ", ".join(format_port(port) for port in instance["missing_ports"]) or "-"
        extra = ", ".join(format_port(port) for port in instance["extra_ports"]) or "-"
        lines.append(
            f"| `{instance['instance_id']}` | `{instance['module_id']}` | {instance['port_count']} | "
            f"{missing} | {extra} | `{instance['path']}` |"
        )

    lines.extend(
        [
            "",
            "## Modules",
            "",
            "| module | default ports | references | missing references |",
            "| --- | ---: | ---: | --- |",
        ]
    )
    for module in report["modules"]:
        missing_refs = ", ".join(f"`{name}`" for name in module["missing_port_refs"]) or "-"
        lines.append(
            f"| `{module['module_id']}` | {module['default_port_count']} | "
            f"{module['port_ref_count']} | {missing_refs} |"
        )

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Audit bundled module ports and optionally an explicitly selected instance directory.")
    parser.add_argument("--instances-root", type=Path, help="Also audit instance configs below this directory; omitted by default.")
    parser.add_argument("--json", dest="json_path", type=Path)
    parser.add_argument("--md", dest="markdown_path", type=Path)
    parser.add_argument("--strict", action="store_true", help="Return a non-zero exit code when failures are found.")
    parser.add_argument("--exclude-hidden", action="store_true", help="Skip instance configs under dot-prefixed paths.")
    parser.add_argument("--repair-missing", action="store_true", help="Append missing module default ports to instance configs.")
    args = parser.parse_args()
    if args.repair_missing and args.instances_root is None:
        parser.error("--repair-missing requires --instances-root")

    report = build_report(
        instances_root=args.instances_root,
        exclude_hidden=args.exclude_hidden,
        repair_missing=args.repair_missing,
    )

    if args.json_path:
        args.json_path.parent.mkdir(parents=True, exist_ok=True)
        args.json_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    if args.markdown_path:
        write_markdown(args.markdown_path, report)

    if not args.json_path and not args.markdown_path:
        print(json.dumps(report["summary"], indent=2, ensure_ascii=False))
        if report["failures"]:
            print()
            for failure in report["failures"]:
                print(f"{failure['type']}: {failure.get('instance_id') or failure.get('module_id') or failure.get('path')}")

    if args.strict and report["failures"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
