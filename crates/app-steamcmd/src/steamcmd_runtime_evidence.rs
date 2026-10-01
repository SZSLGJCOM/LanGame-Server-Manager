/// Only the command's fresh stdout/stderr can prove the console runtime loaded.
/// Bootstrap logs may end with "Verification complete" before the runtime starts.
#[derive(Default)]
pub(super) struct RuntimeEvidence {
    tails: [Vec<u8>; 2],
    console_started: bool,
    api_loaded: bool,
}

impl RuntimeEvidence {
    pub(super) fn push(&mut self, stream: usize, bytes: &[u8]) {
        let Some(tail) = self.tails.get_mut(stream) else {
            return;
        };
        const CONSOLE: &[u8] = b"Steam Console Client (c) Valve Corporation";
        const API: &[u8] = b"Loading Steam API...OK";
        let mut scan = std::mem::take(tail);
        scan.extend_from_slice(bytes);
        self.console_started |= scan.windows(CONSOLE.len()).any(|bytes| bytes == CONSOLE);
        self.api_loaded |= scan.windows(API.len()).any(|bytes| bytes == API);
        *tail = scan[scan.len().saturating_sub(CONSOLE.len().max(API.len()) - 1)..].to_vec();
    }

    pub(super) fn ready(&self) -> bool {
        self.console_started && self.api_loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_requires_both_fresh_console_and_api_markers_across_chunks() {
        let mut evidence = RuntimeEvidence::default();
        evidence.push(
            2,
            b"Steam Console Client (c) Valve Corporation\nLoading Steam API...OK",
        );
        evidence.push(0, b"Verification complete\n");
        assert!(
            !evidence.ready(),
            "bootstrap file records cannot prove runtime readiness"
        );
        evidence.push(0, b"Steam Console Client (c) Valve Corp");
        evidence.push(0, b"oration - version 1788292693\nLoading Steam API...");
        assert!(!evidence.ready());
        evidence.push(0, b"OK\n");
        assert!(evidence.ready());
    }
}
