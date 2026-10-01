//! Test-only admission policy. Reserving an instance does not prove ownership of
//! its processes: each worker must still verify the returned run/session and OS
//! process identities, then complete only after its normal-stop cleanup finishes.
use app_core::{InstanceStatus, InstanceSummary, PortBinding};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize)]
pub(super) struct Ports(BTreeSet<(String, u16)>);

impl Ports {
    fn from_bindings(bindings: &[PortBinding]) -> Result<Self, String> {
        if bindings.is_empty() {
            return Err("acceptance_port_reservations_missing".into());
        }
        let mut ports = BTreeSet::new();
        for binding in bindings {
            let protocol = binding.protocol.trim().to_ascii_lowercase();
            if binding.port == 0 || !matches!(protocol.as_str(), "tcp" | "udp") {
                return Err("acceptance_port_reservation_invalid".into());
            }
            ports.insert((protocol, binding.port));
        }
        Ok(Self(ports))
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        // Existing-instance probes admit only wildcard/loopback bindings. Both
        // overlap on the same protocol/port; TCP and UDP remain separate spaces.
        !self.0.is_disjoint(&other.0)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Candidate {
    pub instance_id: String,
    pub module_id: String,
    pub ports: Ports,
    // The caller may tighten this for large heaps or extra shards. It must not
    // relax an exclusive module without separately establishing its resource use.
    pub exclusive: bool,
}

impl Candidate {
    pub(super) fn from_bindings(
        instance_id: &str,
        module_id: &str,
        bindings: &[PortBinding],
    ) -> Result<Self, String> {
        if instance_id.trim().is_empty() || module_id.trim().is_empty() {
            return Err("acceptance_candidate_identity_missing".into());
        }
        Ok(Self {
            instance_id: instance_id.into(),
            module_id: module_id.into(),
            ports: Ports::from_bindings(bindings)?,
            // The two Forest servers also completed isolated native probes
            // within a 10 GiB whole-test Job peak each on this workstation.
            exclusive: !matches!(
                module_id,
                "terraria"
                    | "necesse"
                    | "barotrauma"
                    | "dontstarve"
                    | "minecraft"
                    | "romestead"
                    | "rimworld"
                    | "corekeeper"
                    | "sevendaystodie"
                    | "unturned"
                    | "vrising"
                    | "sonsoftheforest"
                    | "theforest"
            ),
        })
    }
}

pub(super) struct Scheduler {
    pending: VecDeque<Candidate>,
    active: BTreeMap<String, Candidate>,
    max_parallel: usize,
    halted: bool,
}

impl Scheduler {
    pub(super) fn new(candidates: Vec<Candidate>, max_parallel: usize) -> Result<Self, String> {
        if !(1..=3).contains(&max_parallel) {
            return Err("acceptance_parallel_limit_must_be_between_1_and_3".into());
        }
        let mut seen = BTreeSet::new();
        if candidates
            .iter()
            .any(|candidate| !seen.insert(candidate.instance_id.clone()))
        {
            return Err("acceptance_candidate_instance_duplicated".into());
        }
        Ok(Self {
            pending: candidates.into(),
            active: BTreeMap::new(),
            max_parallel,
            halted: false,
        })
    }

    /// Reserve before polling the worker future. A blocked queue entry may be
    /// skipped, but reservations last through startup, observation and stopping.
    pub(super) fn admit(&mut self) -> Option<Candidate> {
        if self.halted
            || self.active.len() >= self.max_parallel
            || self.active.values().any(|candidate| candidate.exclusive)
        {
            return None;
        }
        let index = self.pending.iter().position(|candidate| {
            (!candidate.exclusive || self.active.is_empty())
                && self
                    .active
                    .values()
                    .all(|active| !candidate.ports.conflicts_with(&active.ports))
        })?;
        let candidate = self.pending.remove(index)?;
        self.active
            .insert(candidate.instance_id.clone(), candidate.clone());
        Some(candidate)
    }

    /// Readiness failure is allowed to continue when cleanup is proven. Unsafe
    /// cleanup permanently closes admission; remaining admitted workers must be
    /// awaited normally rather than cancelled or dropped by the caller.
    pub(super) fn complete(
        &mut self,
        instance_id: &str,
        safe_to_continue: bool,
    ) -> Result<(), String> {
        if self.active.remove(instance_id).is_none() {
            self.halted = true;
            return Err("acceptance_completion_has_no_reservation".into());
        }
        if !safe_to_continue {
            self.halted = true;
        }
        Ok(())
    }

    pub(super) fn active_instance_ids(&self) -> BTreeSet<String> {
        self.active.keys().cloned().collect()
    }

    pub(super) fn active_count(&self) -> usize {
        self.active.len()
    }
    pub(super) fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub(super) fn halted(&self) -> bool {
        self.halted
    }
}

/// Pass only Scheduler::active_instance_ids(), never the entire selected
/// catalog. The target worker separately requires its own instance to be stopped
/// immediately before sending start, and pins identities after the response.
pub(super) fn verify_owned_active(
    instances: &[InstanceSummary],
    owned: &BTreeSet<String>,
) -> Result<(), String> {
    let visible: BTreeSet<_> = instances
        .iter()
        .map(|instance| instance.id.clone())
        .collect();
    if !owned.is_subset(&visible) {
        return Err("acceptance_reserved_instance_disappeared".into());
    }
    if instances
        .iter()
        .any(|instance| !super::selection::is_inactive(instance) && !owned.contains(&instance.id))
    {
        return Err("acceptance_unowned_instance_is_active".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_schedule_forest_servers_share_nonconflicting_slots() {
        let mut scheduler = Scheduler::new(
            vec![
                candidate("forest", "theforest", "udp", 8766),
                candidate("sons", "sonsoftheforest", "udp", 8768),
            ],
            3,
        )
        .unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "forest");
        assert_eq!(scheduler.admit().unwrap().instance_id, "sons");
        assert_eq!(scheduler.active_count(), 2);
        assert!(scheduler.admit().is_none());
        scheduler.complete("forest", true).unwrap();
        scheduler.complete("sons", true).unwrap();
        assert_eq!(scheduler.active_count(), 0);
    }

    fn candidate(id: &str, module: &str, protocol: &str, port: u16) -> Candidate {
        Candidate::from_bindings(
            id,
            module,
            &[PortBinding {
                name: "game".into(),
                protocol: protocol.into(),
                port,
            }],
        )
        .unwrap()
    }

    fn summary(id: &str, status: InstanceStatus, active_process_count: usize) -> InstanceSummary {
        InstanceSummary {
            id: id.into(),
            name: id.into(),
            module_id: "terraria".into(),
            status,
            active_process_count,
            bind_ip: "127.0.0.1".into(),
            port_count: 1,
            autostart: false,
        }
    }

    #[test]
    fn existing_schedule_bounds_parallel_light_instances_and_releases_after_cleanup() {
        let candidates = (1..=4)
            .map(|index| candidate(&format!("i{index}"), "terraria", "tcp", 7000 + index))
            .collect();
        let mut scheduler = Scheduler::new(candidates, 3).unwrap();
        for _ in 0..3 {
            assert!(scheduler.admit().is_some());
        }
        assert!(scheduler.admit().is_none());
        assert_eq!(scheduler.active_count(), 3);
        assert_eq!(scheduler.pending_count(), 1);
        scheduler.complete("i2", true).unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "i4");
    }

    #[test]
    fn existing_schedule_heavy_instances_are_exclusive_in_both_directions() {
        let mut scheduler = Scheduler::new(
            vec![
                candidate("light", "necesse", "udp", 7000),
                candidate("heavy", "rust", "udp", 7001),
                candidate("other", "barotrauma", "udp", 7002),
            ],
            3,
        )
        .unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "light");
        assert_eq!(scheduler.admit().unwrap().instance_id, "other");
        assert!(scheduler.admit().is_none());
        scheduler.complete("light", true).unwrap();
        assert!(scheduler.admit().is_none());
        scheduler.complete("other", true).unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "heavy");
        assert!(scheduler.admit().is_none());

        let mut scheduler = Scheduler::new(
            vec![
                candidate("heavy", "rust", "udp", 7001),
                candidate("light", "necesse", "udp", 7000),
            ],
            3,
        )
        .unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "heavy");
        assert!(scheduler.admit().is_none());
        scheduler.complete("heavy", true).unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "light");
    }

    #[test]
    fn existing_schedule_reserves_all_protocol_ports_and_skips_conflicts() {
        let first = Candidate::from_bindings(
            "first",
            "minecraft",
            &[
                PortBinding {
                    name: "game".into(),
                    protocol: "TCP".into(),
                    port: 7000,
                },
                PortBinding {
                    name: "query".into(),
                    protocol: "udp".into(),
                    port: 7001,
                },
            ],
        )
        .unwrap();
        let mut scheduler = Scheduler::new(
            vec![
                first,
                candidate("conflict", "necesse", "udp", 7001),
                candidate("other-protocol", "terraria", "tcp", 7001),
            ],
            3,
        )
        .unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "first");
        assert_eq!(scheduler.admit().unwrap().instance_id, "other-protocol");
        assert!(scheduler.admit().is_none());
        scheduler.complete("first", true).unwrap();
        assert_eq!(scheduler.admit().unwrap().instance_id, "conflict");
    }

    #[test]
    fn existing_schedule_unsafe_completion_halts_new_admission_but_keeps_other_workers_owned() {
        let mut scheduler = Scheduler::new(
            vec![
                candidate("a", "terraria", "tcp", 1),
                candidate("b", "necesse", "udp", 2),
                candidate("c", "corekeeper", "udp", 3),
            ],
            2,
        )
        .unwrap();
        scheduler.admit().unwrap();
        scheduler.admit().unwrap();
        scheduler.complete("a", false).unwrap();
        assert!(scheduler.halted());
        assert!(scheduler.admit().is_none());
        assert_eq!(
            scheduler.active_instance_ids(),
            BTreeSet::from([String::from("b")])
        );
        assert_eq!(scheduler.pending_count(), 1);
        scheduler.complete("b", true).unwrap();
        assert_eq!(scheduler.active_count(), 0);
        assert!(scheduler.admit().is_none());
    }

    #[test]
    fn existing_schedule_rejects_invalid_limits_duplicate_ids_and_unknown_completion() {
        assert!(Scheduler::new(vec![], 0).is_err());
        assert!(Scheduler::new(vec![], 4).is_err());
        let one = candidate("same", "terraria", "tcp", 1);
        assert!(Scheduler::new(vec![one.clone(), one.clone()], 2).is_err());
        let mut scheduler = Scheduler::new(vec![one], 2).unwrap();
        assert!(scheduler.complete("unknown", true).is_err());
        assert!(scheduler.halted());
        assert!(Candidate::from_bindings("a", "terraria", &[]).is_err());
        for (protocol, port) in [("http", 80), ("tcp", 0)] {
            assert!(
                Candidate::from_bindings(
                    "a",
                    "terraria",
                    &[PortBinding {
                        name: "game".into(),
                        protocol: protocol.into(),
                        port,
                    }]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn existing_schedule_only_admitted_instances_may_be_active() {
        let owned = BTreeSet::from([String::from("owned")]);
        let mut instances = vec![
            summary("owned", InstanceStatus::Running, 1),
            summary("pending", InstanceStatus::Stopped, 0),
        ];
        assert!(verify_owned_active(&instances, &owned).is_ok());
        instances[1].status = InstanceStatus::Error;
        assert!(verify_owned_active(&instances, &owned).is_ok());
        instances[1].active_process_count = 1;
        assert!(verify_owned_active(&instances, &owned).is_err());
        instances[1].active_process_count = 0;
        instances[1].status = InstanceStatus::Starting;
        assert!(verify_owned_active(&instances, &owned).is_err());
        instances[1].status = InstanceStatus::Stopped;
        instances[1].active_process_count = 1;
        assert!(verify_owned_active(&instances, &owned).is_err());
        instances[1].active_process_count = 0;
        assert!(verify_owned_active(&instances, &BTreeSet::new()).is_err());
        assert!(
            verify_owned_active(&instances, &BTreeSet::from([String::from("missing")])).is_err()
        );
    }
}
