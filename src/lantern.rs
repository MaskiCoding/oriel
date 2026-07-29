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
/// Measured with a reader that tells the truth: a parked agent sits at 20 ms
/// per two-second window (median of 76 samples taken while genuinely idle),
/// with a spiky tail reaching 360 ms as it wakes to redraw. A working one runs
/// from 225 ms into the thousands. The tail overlaps the bottom of working, so
/// height alone cannot separate them — see [`CONFIRM`].
pub const THRESHOLD: Duration = Duration::from_millis(150);

/// Consecutive samples over [`THRESHOLD`] before a window lights.
///
/// What separates a parked agent from a working one is not how high it reaches
/// but whether it stays there: idle spikes are single samples, work is
/// sustained. Requiring two in a row costs two seconds of latency at the start
/// of a task and removes the isolated spikes that would otherwise light a
/// window for a full latch over nothing.
const CONFIRM: usize = 2;

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
    /// Consecutive samples each app has been over the threshold for.
    streak: HashMap<i32, usize>,
}

impl Lantern {
    pub fn new(binaries: Vec<String>) -> Self {
        Self {
            binaries,
            prev: Vec::new(),
            lit: HashMap::new(),
            streak: HashMap::new(),
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
            let over = model::by_owner(&lit);
            let at = Instant::now();
            self.streak.retain(|app, _| over.contains_key(app));
            for (app, count) in over {
                let streak = self.streak.entry(app).or_default();
                *streak += 1;
                if *streak >= CONFIRM {
                    self.lit.insert(app, (count, at));
                }
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
