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
///
/// Measured over thirty two-second windows: a working agent burned 180 ms at
/// the lowest, 290 ms at the median; the same agent parked burned 3.5 ms. The
/// gap is fifty-fold, and the old 8 ms sat down in the noise where a chatty
/// MCP server or a stray background child could clear it on its own and light
/// a window with nothing happening in it. This sits twenty times above idle
/// and still leaves more than double the headroom under the quietest real work.
pub const THRESHOLD: Duration = Duration::from_millis(80);

/// An agent waiting on the model burns nothing at all while still very much
/// working — measured, a busy agent burns CPU in every 500 ms window it is
/// computing, and exactly none while a request is in flight. The latch has to
/// outlast that gap or the mark blinks off mid-task, which is the failure that
/// matters: a window that stays lit a few seconds too long is a much smaller
/// lie than one that goes dark while its agent is still thinking.
const LATCH: Duration = Duration::from_secs(15);

/// How long the detector waits between samples.
pub const WINDOW: Duration = Duration::from_millis(2000);

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
