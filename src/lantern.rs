//! Polls the process table and remembers which apps have an agent working.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
    reader: proctable::Reader,
    apps: crate::snapshot::AppPids,
    /// Confirmed agents, with the apps reached through ancestry or sockets.
    lit: HashMap<model::Pid, (Vec<model::Pid>, Instant)>,
    /// Consecutive samples each agent has been over the threshold for.
    streak: HashMap<model::Pid, usize>,
}

impl Lantern {
    pub fn new(binaries: Vec<String>) -> Self {
        Self {
            binaries,
            prev: Vec::new(),
            reader: proctable::Reader::new(),
            apps: crate::snapshot::AppPids::new(),
            lit: HashMap::new(),
            streak: HashMap::new(),
        }
    }

    /// Samples the process table and refreshes the lit set. `roots` are the pids
    /// of apps with windows; work is attributed to the nearest one above it.
    pub fn poll(&mut self) {
        let roots = self.apps.current().to_vec();
        let roots = roots.as_slice();
        let mut now = proctable::table();
        // The whole table, not just processes under app roots: an agent in a
        // detached session sits outside every app tree, and its kernel comm
        // can be a version string rather than the command the user typed, so
        // no cheap pre-filter can find it — only the resolved name can.
        // Measured: 1.7 ms steady state for ~1100 processes, 18 ms on the
        // first sweep while argv resolution is cold.
        let all: Vec<model::Pid> = now.iter().map(|p| p.pid).collect();
        self.reader.detail(&mut now, &all);

        // The first sample has no predecessor to difference against.
        if !self.prev.is_empty() {
            // Only a process under an app root can resolve to an owner, so
            // only those are worth the per-fd socket inspection — the whole
            // table would cost hundreds of processes' fd lists every poll.
            let candidates = model::descendants(&now, roots);
            let mut clients: HashMap<model::Pid, Vec<model::Pid>> = HashMap::new();
            let lit = model::lit_through(
                &self.prev,
                &now,
                &self.binaries,
                THRESHOLD,
                roots,
                |server| {
                    clients
                        .entry(server)
                        .or_insert_with(|| proctable::connected_processes(server, &candidates))
                        .clone()
                },
            );
            self.apply(lit, Instant::now());
        }
        self.prev = now;
    }

    fn apply(&mut self, lit: Vec<model::Lit>, at: Instant) {
        let present: HashSet<model::Pid> = lit.iter().map(|l| l.agent).collect();
        self.streak.retain(|agent, _| present.contains(agent));
        for l in lit {
            let streak = self.streak.entry(l.agent).or_default();
            *streak += 1;
            if *streak >= CONFIRM {
                self.lit.insert(l.agent, (l.owners, at));
            }
        }
        self.lit.retain(|_, (_, seen)| seen.elapsed() < LATCH);
    }

    /// Re-reads which apps are running on the next poll.
    ///
    /// The list is cached because asking is expensive; a summon is the moment
    /// staleness would actually show, so it is refreshed then rather than on a
    /// timer that has no relationship to what the user is looking at.
    pub fn refresh_apps(&mut self) {
        self.apps.invalidate();
    }

    /// Forgets everything measured so far.
    ///
    /// A sample is only meaningful against the one before it. After a gap — a
    /// pause, a config reload — the next delta would span the whole gap and
    /// clear any threshold on work that happened minutes ago, with the streak
    /// still counting from before, so the confirmation that exists to reject
    /// spikes would wave it straight through.
    pub fn reset(&mut self) {
        self.prev.clear();
        self.streak.clear();
        self.lit.clear();
    }

    /// Whether this app has an agent working inside it.
    pub fn working(&self, app: model::Pid) -> bool {
        self.lit.values().any(|(owners, _)| owners.contains(&app))
    }

    /// How many agents are working, including those with no attributable app.
    pub fn count(&self) -> usize {
        self.lit.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe example restates these so it can run without the binary crate.
    /// If they drift it becomes a different detector wearing the same name,
    /// which is exactly what it is meant to check.
    #[test]
    fn the_probe_example_agrees_with_the_shipping_constants() {
        let example = include_str!("../crates/proctable/examples/probe.rs");
        for binary in config::Lantern::default().binaries {
            assert!(
                example.contains(&format!("\"{binary}\"")),
                "probe is missing agent binary {binary}"
            );
        }
        assert!(
            example.contains("from_millis(150)"),
            "probe threshold drifted from THRESHOLD"
        );
        assert!(
            example.contains("from_millis(2000)"),
            "probe window drifted from WINDOW"
        );
    }

    fn own_pid() -> model::Pid {
        model::Pid(i32::try_from(std::process::id()).expect("pids fit in i32 on macOS"))
    }

    /// A table where `agent` sits under `app` and has burned `cpu`.
    fn forest(app: i32, agent: i32, cpu: Duration) -> Vec<model::Proc> {
        vec![
            model::Proc {
                pid: model::Pid(app),
                ppid: model::Pid(0),
                name: "Terminal".into(),
                cpu: Duration::ZERO,
            },
            model::Proc {
                pid: model::Pid(agent),
                ppid: model::Pid(app),
                name: "claude".into(),
                cpu,
            },
        ]
    }

    /// Drives the detector's decision without touching the process table.
    fn decide(samples: &[Duration]) -> Vec<bool> {
        let mut lantern = Lantern::new(vec!["claude".into()]);
        let mut out = Vec::new();
        for cpu in samples {
            let now = forest(1, 2, *cpu);
            if !lantern.prev.is_empty() {
                let lit = model::lit(
                    &lantern.prev,
                    &now,
                    &lantern.binaries,
                    THRESHOLD,
                    &[model::Pid(1)],
                );
                lantern.apply(lit, Instant::now());
            }
            lantern.prev = now;
            out.push(lantern.working(model::Pid(1)));
        }
        out
    }

    #[test]
    fn one_sample_over_the_threshold_is_not_enough() {
        // An idle agent spikes to 360 ms as it wakes to redraw; sustained work
        // is what the mark is for, so a lone spike must not light it.
        let cpu = [
            Duration::ZERO,
            Duration::from_millis(500),
            Duration::from_millis(500),
        ];
        assert_eq!(decide(&cpu), vec![false, false, false]);
    }

    #[test]
    fn two_consecutive_samples_over_the_threshold_light_it() {
        let cpu = [
            Duration::ZERO,
            Duration::from_millis(500),
            Duration::from_millis(1000),
            Duration::from_millis(1500),
        ];
        assert_eq!(decide(&cpu), vec![false, false, true, true]);
    }

    #[test]
    fn a_reset_makes_the_next_sample_meaningless_on_purpose() {
        let mut lantern = Lantern::new(vec!["claude".into()]);
        lantern.prev = forest(1, 2, Duration::from_secs(9));
        lantern.streak.insert(model::Pid(1), 5);
        lantern
            .lit
            .insert(model::Pid(2), (vec![model::Pid(1)], Instant::now()));
        lantern.reset();
        assert!(lantern.prev.is_empty());
        assert!(!lantern.working(model::Pid(1)));
        assert_eq!(lantern.count(), 0);
    }

    #[test]
    fn the_first_poll_cannot_claim_work() {
        // With one sample there is no delta, so nothing may be reported.
        let mut lantern = Lantern::new(vec!["claude".into()]);
        lantern.poll();
        assert_eq!(lantern.count(), 0);
    }

    #[test]
    fn polling_twice_is_harmless_and_stays_bounded() {
        let mut lantern = Lantern::new(vec!["definitely-not-a-real-agent".into()]);
        lantern.poll();
        lantern.poll();
        assert_eq!(lantern.count(), 0);
        assert!(!lantern.working(own_pid()));
    }

    #[test]
    fn ownerless_agents_confirm_latch_and_count_without_lighting_an_app() {
        let mut lantern = Lantern::new(vec!["claude".into()]);
        let ownerless = || model::Lit {
            agent: model::Pid(9),
            name: "claude".into(),
            owners: Vec::new(),
        };

        lantern.apply(vec![ownerless()], Instant::now());
        assert_eq!(lantern.count(), 0, "one sample is not confirmed");
        lantern.apply(vec![ownerless()], Instant::now());
        assert_eq!(lantern.count(), 1);
        assert!(!lantern.working(model::Pid(1)));

        lantern.apply(Vec::new(), Instant::now());
        assert_eq!(lantern.count(), 1, "the confirmed agent remains latched");
    }

    #[test]
    fn one_agent_with_two_viewers_counts_once_and_lights_both_apps() {
        let mut lantern = Lantern::new(vec!["claude".into()]);
        let detected = || model::Lit {
            agent: model::Pid(9),
            name: "claude".into(),
            owners: vec![model::Pid(1), model::Pid(2)],
        };
        lantern.apply(vec![detected()], Instant::now());
        lantern.apply(vec![detected()], Instant::now());
        assert_eq!(lantern.count(), 1);
        assert!(lantern.working(model::Pid(1)));
        assert!(lantern.working(model::Pid(2)));
    }
}
