use std::collections::BTreeMap;

use super::roster::NameRoster;

/// Character ZDO creation owners are session-local peers, never account IDs.
#[derive(Default)]
pub(super) struct ValheimPeers {
    names_by_owner: BTreeMap<i64, Peer>,
    pending_joins: usize,
    pending_departures: usize,
    matched_departure: bool,
}

struct Peer {
    name: String,
    player_key: String,
}

impl ValheimPeers {
    pub fn is_stable(&self) -> bool {
        self.pending_joins == 0 && self.pending_departures == 0
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.names_by_owner
            .values()
            .map(|peer| (peer.player_key.as_str(), peer.name.as_str()))
    }

    fn invalidate(&mut self, roster: &mut NameRoster) {
        *self = Self::default();
        roster.invalidate();
    }

    pub fn apply(&mut self, roster: &mut NameRoster, line: &str) {
        // Accept only the native logger envelope, not chat or historical-save text.
        let Some((stamp, event)) = line.split_once(": ") else {
            return;
        };
        if stamp.len() != 19
            || !stamp
                .bytes()
                .all(|byte| byte.is_ascii_digit() || b"/: ".contains(&byte))
        {
            return;
        }
        let event = event.trim_end();
        if event == "Server: New peer connected,sending global keys" {
            roster.revision = roster.revision.wrapping_add(1);
            self.pending_joins += 1;
            if self.pending_joins > u8::MAX as usize {
                self.invalidate(roster);
            }
        } else if let Some(character) = event.strip_prefix("Got character ZDOID from ") {
            roster.revision = roster.revision.wrapping_add(1);
            let Some((name, id)) = character.rsplit_once(" : ") else {
                self.invalidate(roster);
                return;
            };
            let Some((owner, object)) = parse_zdo_id(id) else {
                self.invalidate(roster);
                return;
            };
            // The native client clears its character during death/respawn; the peer remains online.
            if owner == 0 && object == 0 {
                return;
            }
            if owner == 0 || object == 0 || !valid_name(name) {
                self.invalidate(roster);
                return;
            }
            if !self.names_by_owner.contains_key(&owner) {
                if self.names_by_owner.len() >= u8::MAX as usize {
                    self.invalidate(roster);
                    return;
                }
                self.pending_joins = self.pending_joins.saturating_sub(1);
            }
            if let Some(peer) = self.names_by_owner.get_mut(&owner) {
                remove_name(roster, &peer.name);
                peer.name = name.to_owned();
            } else {
                // This is an opaque row-selection identity for one observed connection.
                // It must survive refresh/respawn but never identify a later owner reuse.
                self.names_by_owner.insert(
                    owner,
                    Peer {
                        name: name.to_owned(),
                        player_key: format!("valheim-log:{}", uuid::Uuid::new_v4().simple()),
                    },
                );
            }
            *roster.names.entry(name.to_owned()).or_default() += 1;
        } else if let Some(abandoned) =
            event.strip_prefix("Destroying abandoned non persistent zdo ")
        {
            let Some((id, owner)) = abandoned.rsplit_once(" owner ") else {
                self.invalidate(roster);
                return;
            };
            let Some(owner) = owner.parse::<i64>().ok().filter(|owner| *owner != 0) else {
                self.invalidate(roster);
                return;
            };
            if parse_zdo_id(id).is_none() {
                self.invalidate(roster);
                return;
            }
            // ZDOMan only emits this after IsPeerConnected(owner) becomes false.
            // Use the actual owner field, not the object's creation owner or a socket's Steam ID.
            if let Some(peer) = self.names_by_owner.remove(&owner) {
                roster.revision = roster.revision.wrapping_add(1);
                remove_name(roster, &peer.name);
                self.pending_departures = self.pending_departures.saturating_sub(1);
                self.matched_departure = true;
            }
        } else if event == "RPC_Disconnect" {
            roster.revision = roster.revision.wrapping_add(1);
            self.pending_departures = self.pending_departures.saturating_add(1);
        } else if event.starts_with("Closing socket ") {
            roster.revision = roster.revision.wrapping_add(1);
            // An abrupt disconnect may omit RPC_Disconnect. Without attributable
            // orphan cleanup, even an unchanged INFO count cannot preserve old names.
            if self.pending_departures > 0
                || (!self.matched_departure && !self.names_by_owner.is_empty())
            {
                self.invalidate(roster);
            }
            self.matched_departure = false;
        } else if event == "Shutting down" || event == "ZNet Shutdown" {
            self.invalidate(roster);
        }
    }
}

fn parse_zdo_id(id: &str) -> Option<(i64, u32)> {
    let (owner, object) = id.split_once(':')?;
    Some((owner.parse().ok()?, object.parse().ok()?))
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= 256
        && !name.chars().any(|ch| ch.is_control() || ch == '\u{fffd}')
}

fn remove_name(roster: &mut NameRoster, name: &str) {
    if let Some(count) = roster.names.get_mut(name) {
        *count -= 1;
        if *count == 0 {
            roster.names.remove(name);
        }
    }
}

#[cfg(test)]
#[path = "valheim_roster_tests.rs"]
mod tests;
