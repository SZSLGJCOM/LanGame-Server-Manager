use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{LAN_MEDIA_PREFIX, MediaSource};

const MAX_LEASES: usize = 4096;
const LEASE_IDLE_TTL: Duration = Duration::from_secs(2 * 60 * 60);

struct Lease {
    identity: String,
    source: MediaSource,
    accessed: Instant,
}

#[derive(Default)]
pub(super) struct MediaLeases {
    entries: Mutex<HashMap<String, Lease>>,
}

impl MediaLeases {
    pub fn register(&self, identity: String, source: MediaSource) -> Result<String, String> {
        self.register_at(identity, source, Instant::now())
    }

    fn register_at(
        &self,
        identity: String,
        source: MediaSource,
        now: Instant,
    ) -> Result<String, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Media lease registry is unavailable")?;
        entries.retain(|_, lease| now.saturating_duration_since(lease.accessed) < LEASE_IDLE_TTL);
        if let Some((id, lease)) = entries.iter_mut().find(|(_, lease)| {
            source.playback.is_none()
                && lease.identity == identity
                && lease.source.purpose == source.purpose
                && lease.source.preference == source.preference
        }) {
            lease.accessed = now;
            return Ok(format!("{LAN_MEDIA_PREFIX}{id}"));
        }
        if entries.len() >= MAX_LEASES {
            let oldest = entries
                .iter()
                .min_by_key(|(_, lease)| lease.accessed)
                .map(|(id, _)| id.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        entries.insert(
            id.clone(),
            Lease {
                identity,
                source,
                accessed: now,
            },
        );
        Ok(format!("{LAN_MEDIA_PREFIX}{id}"))
    }

    pub fn lookup(&self, path: &str) -> Option<MediaSource> {
        self.lookup_at(path, Instant::now())
    }

    fn lookup_at(&self, path: &str, now: Instant) -> Option<MediaSource> {
        let id = path.strip_prefix(LAN_MEDIA_PREFIX)?;
        if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut entries = self.entries.lock().ok()?;
        let lease = entries.get_mut(id)?;
        if now.saturating_duration_since(lease.accessed) >= LEASE_IDLE_TTL {
            entries.remove(id);
            return None;
        }
        lease.accessed = now;
        Some(lease.source.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_cache::MediaPurpose;
    use app_network::SourcePreference;

    fn source(preference: SourcePreference) -> MediaSource {
        MediaSource {
            url: "https://shared.akamai.steamstatic.com/steam/apps/1/header.jpg".into(),
            purpose: MediaPurpose::Image,
            preference,
            playback: None,
        }
    }

    #[test]
    fn opaque_leases_reuse_resource_and_language_without_revealing_management_credentials() {
        let leases = MediaLeases::default();
        let now = Instant::now();
        let china = source(SourcePreference::ChinaFirst);
        let path = leases
            .register_at("resource".into(), china.clone(), now)
            .unwrap();
        assert_eq!(path.len(), LAN_MEDIA_PREFIX.len() + 32);
        assert_eq!(
            leases.register_at("resource".into(), china, now).unwrap(),
            path
        );
        let english = leases
            .register_at(
                "resource".into(),
                source(SourcePreference::InternationalFirst),
                now,
            )
            .unwrap();
        assert_ne!(path, english);
        assert_eq!(
            leases.lookup_at(&path, now).unwrap().preference,
            SourcePreference::ChinaFirst
        );
        assert_eq!(
            leases.lookup_at(&english, now).unwrap().preference,
            SourcePreference::InternationalFirst
        );
        for invalid in [
            format!("{path}?url=private"),
            format!("{path}/../secret"),
            format!("{LAN_MEDIA_PREFIX}00000000000000000000000000000000"),
        ] {
            assert!(leases.lookup_at(&invalid, now).is_none());
        }
    }

    #[test]
    fn leases_expire_after_inactivity_and_registry_capacity_is_bounded() {
        let leases = MediaLeases::default();
        let now = Instant::now();
        let path = leases
            .register_at("old".into(), source(SourcePreference::ChinaFirst), now)
            .unwrap();
        assert!(leases.lookup_at(&path, now + LEASE_IDLE_TTL).is_none());
        for index in 0..=MAX_LEASES {
            leases
                .register_at(
                    index.to_string(),
                    source(SourcePreference::ChinaFirst),
                    now + Duration::from_millis(index as u64),
                )
                .unwrap();
        }
        assert_eq!(leases.entries.lock().unwrap().len(), MAX_LEASES);
    }
}
