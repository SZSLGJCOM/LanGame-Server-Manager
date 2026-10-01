use std::collections::{HashMap, HashSet};

use app_core::{ModulePortGroupSpec, PortBinding};

use crate::{resolve_port_probe_ip, validate_port_binding};

#[cfg(test)]
#[path = "port_remap_tests.rs"]
mod tests;

pub fn remap_taken_port_bindings(
    bind_ip: &str,
    ports: &[PortBinding],
) -> Result<Option<Vec<PortBinding>>, String> {
    remap_taken_port_bindings_for_module("", bind_ip, ports, &[])
}

pub fn remap_taken_port_bindings_for_module(
    module_id: &str,
    bind_ip: &str,
    ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
) -> Result<Option<Vec<PortBinding>>, String> {
    if ports.is_empty() {
        return Ok(None);
    }
    let probe_ip = resolve_port_probe_ip(bind_ip)?;
    remap_with_probe(module_id, ports, port_groups, |port| {
        validate_port_binding(probe_ip, port).is_none()
    })
}

struct AllocationUnit {
    members: Vec<(usize, u16)>,
    base: u16,
    maximum_offset: u16,
}

fn remap_with_probe(
    module_id: &str,
    ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
    mut available: impl FnMut(&PortBinding) -> bool,
) -> Result<Option<Vec<PortBinding>>, String> {
    let mut normalized = ports.to_vec();
    for port in &mut normalized {
        port.protocol = port.protocol.trim().to_ascii_lowercase();
    }
    let mut indexes = HashMap::new();
    let mut endpoints = HashSet::new();
    for (index, port) in normalized.iter().enumerate() {
        if !matches!(port.protocol.as_str(), "tcp" | "udp") {
            return Err(format!(
                "Unsupported port protocol '{}' for '{}'.",
                port.protocol, port.name
            ));
        }
        if indexes.insert(port.name.as_str(), index).is_some() {
            return Err(format!("Duplicate port binding name '{}'.", port.name));
        }
        if !endpoints.insert((port.protocol.clone(), port.port)) {
            return Err(format!(
                "Duplicate port binding '{}' ({}:{}) is invalid; protocol and port must be unique.",
                port.name, port.protocol, port.port
            ));
        }
    }
    let units = allocation_units(&normalized, &indexes, port_groups)?;
    let mut remapped = normalized.clone();
    let mut reserved = HashSet::new();
    for unit in units {
        let requested = &normalized[unit.members[0].0];
        if unit.base == 0 {
            for (index, _) in &unit.members {
                reserved.insert((normalized[*index].protocol.clone(), 0));
            }
            continue;
        }
        let range = if unit.members.len() == 1 {
            automatic_startup_port_remap_range(module_id, requested)
        } else {
            None
        };
        let (minimum, maximum, wraps) = range
            .map(|(minimum, maximum)| (minimum, maximum, true))
            .unwrap_or((1, u16::MAX - unit.maximum_offset, false));
        if unit.base > maximum && !wraps {
            return Err(exhausted_error(requested, range));
        }
        let mut candidate = unit.base.clamp(minimum, maximum);
        let first_candidate = candidate;
        loop {
            // Probe the complete transport block before reserving any member.
            // Moving only a peer/query port cannot change the game's derived port.
            let all_available = unit.members.iter().all(|(index, offset)| {
                let binding = PortBinding {
                    port: candidate + offset,
                    ..normalized[*index].clone()
                };
                !reserved.contains(&(binding.protocol.clone(), binding.port)) && available(&binding)
            });
            if all_available {
                for (index, offset) in &unit.members {
                    remapped[*index].port = candidate + offset;
                    reserved.insert((remapped[*index].protocol.clone(), remapped[*index].port));
                }
                break;
            }
            if candidate == maximum {
                if !wraps || minimum == first_candidate {
                    return Err(exhausted_error(requested, range));
                }
                candidate = minimum;
            } else {
                candidate += 1;
            }
            if candidate == first_candidate {
                return Err(exhausted_error(requested, range));
            }
        }
    }
    let changed = remapped
        .iter()
        .zip(ports)
        .any(|(next, previous)| next.port != previous.port || next.protocol != previous.protocol);
    Ok(changed.then_some(remapped))
}

fn allocation_units(
    ports: &[PortBinding],
    indexes: &HashMap<&str, usize>,
    groups: &[ModulePortGroupSpec],
) -> Result<Vec<AllocationUnit>, String> {
    let mut owners = HashSet::new();
    let mut units = Vec::new();
    for group in groups {
        let invalid = |reason: &str| format!("Invalid port group '{}': {reason}.", group.id);
        if group.members.is_empty() {
            return Err(invalid("no member bindings"));
        }
        if let Some(offsets) = &group.member_offsets
            && (offsets.len() != group.members.len()
                || group.members.iter().any(|name| !offsets.contains_key(name)))
        {
            return Err(invalid("offsets must name every member exactly once"));
        }
        let mut members = Vec::new();
        let mut shapes = HashSet::new();
        for name in &group.members {
            let index = *indexes
                .get(name.as_str())
                .ok_or_else(|| invalid("missing member binding"))?;
            if !owners.insert(index) {
                return Err(invalid(
                    "a binding belongs to multiple groups or is repeated",
                ));
            }
            let offset = group
                .member_offsets
                .as_ref()
                .and_then(|offsets| offsets.get(name))
                .copied()
                .unwrap_or(0);
            if !shapes.insert((ports[index].protocol.as_str(), offset)) {
                return Err(invalid("members overlap at the same protocol and offset"));
            }
            members.push((index, offset));
        }
        let maximum_offset = members.iter().map(|(_, offset)| *offset).max().unwrap_or(0);
        let (primary, offset) = members[0];
        let base = ports[primary]
            .port
            .checked_sub(offset)
            .ok_or_else(|| invalid("primary port is below its offset"))?;
        if base == 0
            && (maximum_offset != 0 || members.iter().any(|(index, _)| ports[*index].port != 0))
        {
            return Err(invalid("only an entire zero-offset group can be disabled"));
        }
        // The first declared member owns the base, matching the UI. Reconcile
        // stored bindings created before a module declared its native offsets.
        units.push(AllocationUnit {
            members,
            base,
            maximum_offset,
        });
    }
    for (index, port) in ports.iter().enumerate() {
        if !owners.contains(&index) {
            units.push(AllocationUnit {
                members: vec![(index, 0)],
                base: port.port,
                maximum_offset: 0,
            });
        }
    }
    units.sort_by_key(|unit| {
        unit.members
            .iter()
            .map(|(index, _)| *index)
            .min()
            .unwrap_or(0)
    });
    Ok(units)
}

fn exhausted_error(requested: &PortBinding, range: Option<(u16, u16)>) -> String {
    if range.is_some() {
        format!(
            "No free {} port is available for {} in Klei's 10998-11018 DST LAN discovery range",
            requested.protocol, requested.name
        )
    } else {
        format!(
            "No free {} port block is available for {} starting from {}",
            requested.protocol, requested.name, requested.port
        )
    }
}

fn automatic_startup_port_remap_range(module_id: &str, port: &PortBinding) -> Option<(u16, u16)> {
    if module_id == "dontstarve"
        && port.protocol.eq_ignore_ascii_case("udp")
        && app_core::dst_shards::DST_SHARDS
            .iter()
            .any(|shard| shard.game_port == port.name)
        && (10_998..=11_018).contains(&port.port)
    {
        Some((10_998, 11_018))
    } else {
        None
    }
}
