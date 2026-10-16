use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct NameRoster {
    pub names: BTreeMap<String, usize>,
    pub revision: u64,
    pub incomplete: bool,
}

impl NameRoster {
    pub fn count(&self) -> usize {
        self.names.values().sum()
    }

    pub fn invalidate(&mut self) {
        self.names.clear();
        self.incomplete = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn establish_empty(&mut self) {
        self.names.clear();
        self.incomplete = false;
    }

    pub fn apply(&mut self, line: &str) {
        let Some((_, event)) = line.split_once("[server] ") else {
            return;
        };
        let event = event.trim_end();
        let join = event
            .strip_prefix("Player '")
            .and_then(|text| text.strip_suffix("' logged in with Permissions:"));
        let leave = event
            .strip_prefix("Remove Player '")
            .and_then(|text| text.strip_suffix('\''));
        let Some(name) = join.or(leave) else {
            if event.starts_with("Remove Player ")
                || (event.starts_with("Player '") && event.contains("logged"))
            {
                self.invalidate();
            }
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        if name.trim().is_empty()
            || name.chars().count() > 256
            || name.chars().any(|ch| ch.is_control() || ch == '\u{fffd}')
        {
            self.invalidate();
            return;
        }
        if join.is_some() {
            // A2S encodes its player count in one byte. Do not retain an
            // unbounded list if an incomplete native log misses departures.
            if self.count() >= u8::MAX as usize {
                self.invalidate();
            } else {
                *self.names.entry(name.to_owned()).or_default() += 1;
            }
        } else if let Some(count) = self.names.get_mut(name) {
            *count -= 1;
            if *count == 0 {
                self.names.remove(name);
            }
        } else {
            self.invalidate();
        }
    }
}

#[cfg(test)]
#[path = "enshrouded_roster_tests.rs"]
mod tests;
