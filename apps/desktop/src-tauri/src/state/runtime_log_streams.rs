use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct RuntimeLogStreams {
    active: HashMap<(String, String), StreamEntry>,
    next_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLogStreamLease {
    generation: u64,
}

#[derive(Debug)]
struct StreamEntry {
    lease: RuntimeLogStreamLease,
    producer_finished: bool,
    owner: Option<(String, RuntimeLogStreamLease)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLogStreamStatus {
    Active,
    ProducerFinished,
    Cancelled,
}

impl RuntimeLogStreams {
    pub fn reserve(&mut self, instance_id: &str, log_path: &str) -> Option<RuntimeLogStreamLease> {
        let key = stream_key(instance_id, log_path);
        if self
            .active
            .get(&key)
            .is_some_and(|entry| !entry.producer_finished)
        {
            return None;
        }
        self.next_generation = self.next_generation.checked_add(1)?;
        let lease = RuntimeLogStreamLease {
            generation: self.next_generation,
        };
        // A replacement run may use a different filename. Retire completed
        // streams before its first event; active sibling shards keep their leases.
        self.active
            .retain(|(id, _), entry| id != instance_id || !entry.producer_finished);
        self.active.insert(
            key,
            StreamEntry {
                lease,
                producer_finished: false,
                owner: None,
            },
        );
        Some(lease)
    }

    /// A game's native stream follows the exact console producer generation.
    pub fn reserve_linked(
        &mut self,
        instance_id: &str,
        log_path: &str,
        console_path: &str,
    ) -> Option<RuntimeLogStreamLease> {
        let owner_key = stream_key(instance_id, console_path);
        if owner_key == stream_key(instance_id, log_path) {
            return None;
        }
        let owner = self.active.get(&owner_key)?;
        if owner.producer_finished {
            return None;
        }
        let owner = (owner_key.1, owner.lease);
        let key = stream_key(instance_id, log_path);
        if self
            .active
            .get(&key)
            .is_some_and(|entry| entry.owner.as_ref() == Some(&owner))
        {
            return None;
        }
        // Attaching a viewer is not a new run. Do not retire a sibling map's
        // finished producer before that map has drained its final output.
        self.next_generation = self.next_generation.checked_add(1)?;
        let lease = RuntimeLogStreamLease {
            generation: self.next_generation,
        };
        self.active.insert(
            key,
            StreamEntry {
                lease,
                producer_finished: false,
                owner: Some(owner),
            },
        );
        Some(lease)
    }

    pub fn release(
        &mut self,
        instance_id: &str,
        log_path: &str,
        lease: RuntimeLogStreamLease,
    ) -> bool {
        if !self.is_active(instance_id, log_path, lease) {
            return false;
        }
        let key = stream_key(instance_id, log_path);
        self.active.remove(&key);
        // Finished children own their final drain; an unexpectedly ended parent
        // cannot leave an active native watcher behind.
        self.active.retain(|(id, _), entry| {
            id != instance_id
                || entry.producer_finished
                || entry.owner.as_ref() != Some(&(key.1.clone(), lease))
        });
        true
    }

    pub fn release_instance(&mut self, instance_id: &str) {
        self.active.retain(|(id, _), _| id != instance_id);
    }

    pub fn release_path(&mut self, instance_id: &str, log_path: &str) {
        let key = stream_key(instance_id, log_path);
        self.active.remove(&key);
        self.active.retain(|(id, _), entry| {
            id != instance_id || entry.owner.as_ref().is_none_or(|(path, _)| *path != key.1)
        });
    }

    /// Only the owner that has closed the producer may request final draining.
    pub fn finish_path(&mut self, instance_id: &str, log_path: &str) {
        let key = stream_key(instance_id, log_path);
        let Some(owner) = self
            .active
            .get(&key)
            .map(|entry| (key.1.clone(), entry.lease))
        else {
            return;
        };
        for ((id, path), entry) in &mut self.active {
            if id == instance_id && (*path == key.1 || entry.owner.as_ref() == Some(&owner)) {
                entry.producer_finished = true;
            }
        }
    }

    pub fn status(
        &self,
        instance_id: &str,
        log_path: &str,
        lease: RuntimeLogStreamLease,
    ) -> RuntimeLogStreamStatus {
        match self.active.get(&stream_key(instance_id, log_path)) {
            Some(entry) if entry.lease == lease && entry.producer_finished => {
                RuntimeLogStreamStatus::ProducerFinished
            }
            Some(entry) if entry.lease == lease => RuntimeLogStreamStatus::Active,
            _ => RuntimeLogStreamStatus::Cancelled,
        }
    }

    pub fn is_active(
        &self,
        instance_id: &str,
        log_path: &str,
        lease: RuntimeLogStreamLease,
    ) -> bool {
        self.status(instance_id, log_path, lease) != RuntimeLogStreamStatus::Cancelled
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub fn paths(&self) -> Vec<(String, String)> {
        self.active.keys().cloned().collect()
    }
}

fn stream_key(instance_id: &str, log_path: &str) -> (String, String) {
    (
        instance_id.to_owned(),
        log_path.replace('\\', "/").to_ascii_lowercase(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_log_streams_follow_their_map_producer_and_drain_after_completion() {
        let mut streams = RuntimeLogStreams::default();
        let console = streams.reserve("ark", "run-1.log").unwrap();
        let sibling = streams.reserve("ark", "run-2.log").unwrap();
        let native = streams
            .reserve_linked("ark", "island.log", "run-1.log")
            .unwrap();
        let other = streams
            .reserve_linked("ark", "desert.log", "run-2.log")
            .unwrap();
        assert!(
            streams
                .reserve_linked("ark", "island.log", "run-1.log")
                .is_none()
        );
        assert!(
            streams
                .reserve_linked("other-instance", "island.log", "run-1.log")
                .is_none()
        );
        streams.finish_path("ark", "run-1.log");
        assert_eq!(
            streams.status("ark", "island.log", native),
            RuntimeLogStreamStatus::ProducerFinished
        );
        assert_eq!(
            streams.status("ark", "desert.log", other),
            RuntimeLogStreamStatus::Active
        );
        assert!(streams.release("ark", "run-1.log", console));
        assert_eq!(
            streams.status("ark", "island.log", native),
            RuntimeLogStreamStatus::ProducerFinished
        );
        assert!(streams.release("ark", "island.log", native));
        // Reader failure and explicit cancellation cannot leave orphan watchers.
        assert!(streams.release("ark", "run-2.log", sibling));
        assert_eq!(
            streams.status("ark", "desert.log", other),
            RuntimeLogStreamStatus::Cancelled
        );
        assert!(streams.is_empty());
    }

    #[test]
    fn game_log_attachment_does_not_retire_another_maps_final_drain() {
        let mut streams = RuntimeLogStreams::default();
        let first = streams.reserve("ark", "a.log").unwrap();
        streams.reserve("ark", "b.log").unwrap();
        let native = streams
            .reserve_linked("ark", "native-a.log", "a.log")
            .unwrap();
        streams.finish_path("ark", "a.log");
        streams
            .reserve_linked("ark", "native-b.log", "b.log")
            .unwrap();
        assert_eq!(
            streams.status("ark", "a.log", first),
            RuntimeLogStreamStatus::ProducerFinished
        );
        assert_eq!(
            streams.status("ark", "native-a.log", native),
            RuntimeLogStreamStatus::ProducerFinished
        );
    }

    #[test]
    fn game_log_stream_replacement_cancels_old_generation_and_rejects_missing_owner() {
        let mut streams = RuntimeLogStreams::default();
        assert!(
            streams
                .reserve_linked("ark", "native.log", "missing.log")
                .is_none()
        );
        streams.reserve("ark", "old.log").unwrap();
        let old = streams
            .reserve_linked("ark", "native.log", "old.log")
            .unwrap();
        streams.finish_path("ark", "old.log");
        assert!(
            streams
                .reserve_linked("ark", "native.log", "old.log")
                .is_none()
        );
        streams.reserve("ark", "new.log").unwrap();
        let current = streams
            .reserve_linked("ark", "native.log", "new.log")
            .unwrap();
        assert_eq!(
            streams.status("ark", "native.log", old),
            RuntimeLogStreamStatus::Cancelled
        );
        assert!(!streams.release("ark", "native.log", old));
        assert_eq!(
            streams.status("ark", "native.log", current),
            RuntimeLogStreamStatus::Active
        );
        streams.release_path("ark", "new.log");
        assert!(streams.is_empty());
    }

    #[test]
    fn completion_keeps_the_lease_until_the_tail_is_drained() {
        let mut streams = RuntimeLogStreams::default();
        let lease = streams.reserve("world", "caves.log").unwrap();
        streams.finish_path("world", "caves.log");
        assert_eq!(
            streams.status("world", "caves.log", lease),
            RuntimeLogStreamStatus::ProducerFinished
        );
        assert!(streams.is_active("world", "caves.log", lease));
        assert!(streams.release("world", "caves.log", lease));
        assert!(streams.is_empty());
    }

    #[test]
    fn replacement_and_cancellation_do_not_flush_an_old_generation() {
        let mut streams = RuntimeLogStreams::default();
        let old = streams.reserve("world", "server.log").unwrap();
        streams.finish_path("world", "server.log");
        let new = streams.reserve("world", "server.log").unwrap();
        assert_eq!(
            streams.status("world", "server.log", old),
            RuntimeLogStreamStatus::Cancelled
        );
        assert!(!streams.release("world", "server.log", old));
        assert_eq!(
            streams.status("world", "server.log", new),
            RuntimeLogStreamStatus::Active
        );
        streams.release_instance("world");
        assert_eq!(
            streams.status("world", "server.log", new),
            RuntimeLogStreamStatus::Cancelled
        );
    }

    #[test]
    fn a_new_path_retires_finished_streams_without_cancelling_live_shards() {
        let mut streams = RuntimeLogStreams::default();
        let old = streams.reserve("world", "old-run.log").unwrap();
        let sibling = streams.reserve("world", "caves.log").unwrap();
        streams.finish_path("world", "old-run.log");
        let new = streams.reserve("world", "new-run.log").unwrap();
        assert_eq!(
            streams.status("world", "old-run.log", old),
            RuntimeLogStreamStatus::Cancelled
        );
        assert_eq!(
            streams.status("world", "new-run.log", new),
            RuntimeLogStreamStatus::Active
        );
        assert_eq!(
            streams.status("world", "caves.log", sibling),
            RuntimeLogStreamStatus::Active
        );
    }
}
