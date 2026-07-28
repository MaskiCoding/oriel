//! Polls the process table and remembers which apps have an agent working.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The commands worth watching for. A user can add their own.
pub const DEFAULT_BINARIES: [&str; 6] = [
    "claude",
    "cursor-agent",
    "codex",
    "aider",
    "gemini",
    "opencode",
];

/// CPU an agent's subtree must burn between samples to count as working.
/// Measured: a working agent burns hundreds of milliseconds per second, a
/// parked one burns exactly none, so the gap this sits in is enormous.
const THRESHOLD: Duration = Duration::from_millis(8);

/// An agent waiting on a slow network call burns nothing while still working.
/// Staying lit briefly past the last real work keeps the mark steady instead of
/// blinking through every pause.
const LATCH: Duration = Duration::from_secs(6);

pub struct Lantern {
    binaries: Vec<String>,
    prev: Vec<model::Proc>,
    lit: HashMap<i32, (usize, Instant)>,
}

impl Lantern {
    pub fn new(binaries: Vec<String>) -> Self {
        Self {
            binaries,
            prev: Vec::new(),
            lit: HashMap::new(),
        }
    }

    /// Samples the process table and refreshes the lit set. `roots` are the pids
    /// of apps with windows; work is attributed to the nearest one above it.
    pub fn poll(&mut self, roots: &[i32]) {
        let mut now = proctable::table();
        let under = model::descendants(&now, roots);
        proctable::detail(&mut now, &under);

        // The first sample has no predecessor to difference against.
        if !self.prev.is_empty() {
            let lit = model::lit(&self.prev, &now, &self.binaries, THRESHOLD, roots);
            let at = Instant::now();
            for (app, count) in model::by_owner(&lit) {
                self.lit.insert(app, (count, at));
            }
            self.lit.retain(|_, (_, seen)| seen.elapsed() < LATCH);
        }
        self.prev = now;
    }

    /// Whether this app has an agent working inside it.
    pub fn working(&self, app: i32) -> bool {
        self.lit.contains_key(&app)
    }

    /// How many agents are working across every app.
    pub fn count(&self) -> usize {
        self.lit.values().map(|(n, _)| *n).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own_pid() -> i32 {
        i32::try_from(std::process::id()).expect("pids fit in i32 on macOS")
    }

    #[test]
    fn the_first_poll_cannot_claim_work() {
        // With one sample there is no delta, so nothing may be reported.
        let mut lantern = Lantern::new(vec!["claude".into()]);
        lantern.poll(&[own_pid()]);
        assert_eq!(lantern.count(), 0);
    }

    #[test]
    fn polling_twice_is_harmless_and_stays_bounded() {
        let mut lantern = Lantern::new(vec!["definitely-not-a-real-agent".into()]);
        let roots = [own_pid()];
        lantern.poll(&roots);
        lantern.poll(&roots);
        assert_eq!(lantern.count(), 0);
        assert!(!lantern.working(roots[0]));
    }
}
