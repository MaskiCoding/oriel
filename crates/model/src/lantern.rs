//! Lantern: which windows have an agent burning work inside them.
//!
//! Agent CLIs announce their state in the window title, but that title lies in
//! both directions — the same braille spinner means "waiting for input" and
//! "running a tool", and a covered window stops redrawing so the last title can
//! be minutes stale. CPU time cannot lie the same way: a waiting agent burns
//! none, a working one burns some, and neither depends on being on screen.
//!
//! Work counts across the tools an agent has spawned, because an agent running
//! a tool sits idle while the tool burns — the exact case the title spinner got
//! wrong. The walk stops at any nested agent, so an orchestrator does not claim
//! its worker's effort.
//!
//! Two things this cannot see. A tool that starts and finishes between samples
//! leaves no process behind to measure, so very short work goes unnoticed. And
//! attribution reaches the app a window belongs to, never the pane inside it.

use std::collections::{HashMap, HashSet};

use crate::Pid;
use std::time::Duration;

/// One process as the kernel reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: Pid,
    pub ppid: Pid,
    pub name: String,
    /// CPU time burned since this process started.
    pub cpu: Duration,
}

/// An agent found to be working, and the apps whose windows should light up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lit {
    pub agent: Pid,
    pub name: String,
    /// App ancestors whose windows should light. Empty means unattributable.
    pub owners: Vec<Pid>,
}

/// Agents that burned more than `threshold` of CPU across their subtree between
/// the two samples. `roots` are the pids of apps Oriel can see windows for;
/// each hit is attributed to the nearest ancestor among them.
///
/// Attribution is app-level by nature. A terminal multiplexer runs every pane
/// under one window, and panes are invisible to the window server, so the
/// honest claim is "this app has an agent working", never which pane.
pub fn lit(
    before: &[Proc],
    after: &[Proc],
    binaries: &[String],
    threshold: Duration,
    roots: &[Pid],
) -> Vec<Lit> {
    lit_through(before, after, binaries, threshold, roots, |_| Vec::new())
}

/// Like [`lit`], resolving a detached process tree through the processes that
/// hold sockets connected to its topmost process below launchd.
///
/// Socket inspection stays outside the model: the callback supplies process
/// ids, while this function owns ancestry, app attribution, and deduplication.
pub fn lit_through(
    before: &[Proc],
    after: &[Proc],
    binaries: &[String],
    threshold: Duration,
    roots: &[Pid],
    mut connected: impl FnMut(Pid) -> Vec<Pid>,
) -> Vec<Lit> {
    let was: HashMap<Pid, Duration> = before.iter().map(|p| (p.pid, p.cpu)).collect();
    let parent: HashMap<Pid, Pid> = after.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for p in after {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let now: HashMap<Pid, Duration> = after.iter().map(|p| (p.pid, p.cpu)).collect();

    let is_agent: HashSet<Pid> = after
        .iter()
        .filter(|p| binaries.iter().any(|b| b == &p.name))
        .map(|p| p.pid)
        .collect();

    let mut out: Vec<Lit> = after
        .iter()
        .filter(|p| is_agent.contains(&p.pid))
        .filter(|p| burned(p.pid, &children, &now, &was, &is_agent) > threshold)
        .map(|p| {
            let mut owners: Vec<Pid> = owner_of(p.pid, &parent, roots).into_iter().collect();
            if owners.is_empty() {
                let server = detached_root(p.pid, &parent);
                owners.extend(
                    connected(server)
                        .into_iter()
                        .filter_map(|client| owner_of(client, &parent, roots)),
                );
                owners.sort_unstable();
                owners.dedup();
            }
            Lit {
                agent: p.pid,
                name: p.name.clone(),
                owners,
            }
        })
        .collect();
    out.sort_by_key(|l| l.agent);
    out
}

/// Topmost process in `pid`'s tree before launchd (pid 1) or a missing link.
fn detached_root(pid: Pid, parent: &HashMap<Pid, Pid>) -> Pid {
    let mut cur = pid;
    let mut seen = HashSet::new();
    while seen.insert(cur) {
        match parent.get(&cur) {
            Some(&next) if next != cur && next.0 > 1 => cur = next,
            _ => break,
        }
    }
    cur
}

/// CPU burned by `pid` and the tools under it. A process that appeared since
/// the last sample counts all of its time: a tool that spawned and ran is work.
///
/// The walk stops at any *other* agent, so an agent that merely sits waiting on
/// a nested one is dark while the nested one is lit. Without this an orchestrator
/// and each of its workers would all claim the same work.
fn burned(
    pid: Pid,
    children: &HashMap<Pid, Vec<Pid>>,
    now: &HashMap<Pid, Duration>,
    was: &HashMap<Pid, Duration>,
    agents: &HashSet<Pid>,
) -> Duration {
    let mut total = Duration::ZERO;
    let mut stack = vec![pid];
    // A ppid chain can in principle cycle. Counting steps bounds the walk but
    // still lets the same process be summed repeatedly on the way round, which
    // inflates the total; remembering what has been counted is what makes the
    // sum right rather than merely finite.
    let mut seen: HashSet<Pid> = HashSet::new();
    while let Some(p) = stack.pop() {
        if !seen.insert(p) {
            continue;
        }
        let cur = now.get(&p).copied().unwrap_or_default();
        total += cur.saturating_sub(was.get(&p).copied().unwrap_or_default());
        if let Some(kids) = children.get(&p) {
            stack.extend(
                kids.iter()
                    .copied()
                    .filter(|k| *k != p && !agents.contains(k)),
            );
        }
    }
    total
}

/// Nearest ancestor of `pid` that is one of `roots`.
fn owner_of(pid: Pid, parent: &HashMap<Pid, Pid>, roots: &[Pid]) -> Option<Pid> {
    let mut cur = pid;
    for _ in 0..parent.len() {
        if roots.contains(&cur) {
            return Some(cur);
        }
        match parent.get(&cur) {
            Some(&next) if next != cur && next.0 > 0 => cur = next,
            _ => return None,
        }
    }
    None
}

/// What an agent's subtree actually burned between two samples — the number
/// [`lit`] compares against its threshold. Exposed so a window that is not
/// lighting up can be told apart from an agent that is not working.
pub fn burn(before: &[Proc], after: &[Proc], pid: Pid, binaries: &[String]) -> Duration {
    let was: HashMap<Pid, Duration> = before.iter().map(|p| (p.pid, p.cpu)).collect();
    let now: HashMap<Pid, Duration> = after.iter().map(|p| (p.pid, p.cpu)).collect();
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for p in after {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let agents: HashSet<Pid> = after
        .iter()
        .filter(|p| binaries.iter().any(|b| b == &p.name))
        .map(|p| p.pid)
        .collect();
    burned(pid, &children, &now, &was, &agents)
}

/// Every process running under any of `roots`, roots included. Callers use this
/// to resolve true executable names for the few processes that could be agents
/// instead of the whole table, which costs a syscall each.
pub fn descendants(table: &[Proc], roots: &[Pid]) -> Vec<Pid> {
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for p in table {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    // Membership by set, not by scanning what has been collected so far: the
    // walk covers every process under every app, so a linear check turns each
    // poll into a quadratic sweep of the whole table.
    let mut seen: HashSet<Pid> = HashSet::new();
    let mut out = Vec::new();
    let mut stack: Vec<Pid> = roots.to_vec();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        out.push(pid);
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied().filter(|k| *k != pid));
        }
    }
    out.sort_unstable();
    out
}

/// Apps with at least one working agent, and how many.
pub fn by_owner(lit: &[Lit]) -> HashMap<Pid, usize> {
    let mut out: HashMap<Pid, usize> = HashMap::new();
    for owner in lit.iter().flat_map(|l| l.owners.iter().copied()) {
        *out.entry(owner).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, parent: i32, name: &str, ms: u64) -> Proc {
        Proc {
            pid: Pid(pid),
            ppid: Pid(parent),
            name: name.into(),
            cpu: Duration::from_millis(ms),
        }
    }

    fn agents() -> Vec<String> {
        vec!["claude".to_string(), "cursor-agent".to_string()]
    }

    fn tick() -> Duration {
        Duration::from_millis(10)
    }

    #[test]
    fn an_agent_burning_cpu_is_lit() {
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 400)];
        let out = lit(&before, &after, &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].agent, Pid(2));
        assert_eq!(out[0].owners, vec![Pid(1)]);
    }

    #[test]
    fn an_agent_waiting_for_input_is_dark() {
        // The measured idle case: a parked agent burns exactly nothing.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        assert!(lit(&before, &after, &agents(), tick(), &[Pid(1)]).is_empty());
    }

    #[test]
    fn work_in_a_spawned_tool_counts_as_the_agent_working() {
        // The case the title spinner got wrong: agent idle, tool burning.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        let after = vec![
            p(1, 0, "ghostty", 0),
            p(2, 1, "claude", 100),
            p(3, 2, "rg", 250),
        ];
        let out = lit(&before, &after, &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1, "subtree work must light the agent");
        assert_eq!(out[0].agent, Pid(2));
    }

    #[test]
    fn an_orchestrator_waiting_on_a_worker_is_dark_and_only_the_worker_is_lit() {
        // The fleet shape: a parent agent blocked on a nested one. Counting both
        // would report two working agents for one piece of work.
        let table = |inner| {
            vec![
                p(1, 0, "ghostty", 0),
                p(2, 1, "claude", 500),
                p(3, 2, "cursor-agent", inner),
            ]
        };
        let out = lit(&table(0), &table(400), &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1, "only the nested worker is doing the work");
        assert_eq!(out[0].agent, Pid(3));
        assert_eq!(by_owner(&out).get(&Pid(1)), Some(&1));
    }

    #[test]
    fn an_orchestrator_doing_its_own_work_is_still_lit() {
        let table = |outer| {
            vec![
                p(1, 0, "ghostty", 0),
                p(2, 1, "claude", outer),
                p(3, 2, "cursor-agent", 0),
            ]
        };
        let out = lit(&table(0), &table(400), &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].agent, Pid(2));
    }

    #[test]
    fn a_tool_that_finishes_between_samples_is_missed() {
        // A known blind spot, pinned so it is a decision rather than a surprise:
        // work is measured from processes still alive at the second sample, so a
        // tool that both starts and exits inside one window leaves nothing to
        // measure. Recording it here means a future change that fixes it will
        // fail this test loudly instead of silently drifting.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        assert!(
            lit(&before, &after, &agents(), tick(), &[Pid(1)]).is_empty(),
            "a vanished tool leaves no evidence behind"
        );
    }

    #[test]
    fn a_busy_non_agent_is_ignored() {
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "cargo", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "cargo", 9_000)];
        assert!(lit(&before, &after, &agents(), tick(), &[Pid(1)]).is_empty());
    }

    #[test]
    fn work_under_a_multiplexer_still_reaches_the_app() {
        // herdr's real shape: app -> shell -> herdr -> pane shell -> agent.
        let chain = |cpu| {
            vec![
                p(1, 0, "ghostty", 0),
                p(2, 1, "login", 0),
                p(3, 2, "fish", 0),
                p(4, 3, "herdr", 0),
                p(5, 4, "fish", 0),
                p(6, 5, "claude", cpu),
            ]
        };
        let out = lit(&chain(100), &chain(500), &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].owners,
            vec![Pid(1)],
            "must attribute up to the terminal app"
        );
    }

    #[test]
    fn two_panes_in_one_window_count_separately_but_share_an_owner() {
        let both = |a, b| {
            vec![
                p(1, 0, "ghostty", 0),
                p(2, 1, "claude", a),
                p(3, 1, "cursor-agent", b),
            ]
        };
        let out = lit(&both(0, 0), &both(300, 300), &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 2);
        assert_eq!(by_owner(&out).get(&Pid(1)), Some(&2));
    }

    #[test]
    fn descendants_reach_through_a_multiplexer_and_stop_at_the_tree() {
        let table = vec![
            p(1, 0, "ghostty", 0),
            p(2, 1, "fish", 0),
            p(3, 2, "herdr", 0),
            p(4, 3, "claude", 0),
            p(9, 0, "Safari", 0),
        ];
        assert_eq!(
            descendants(&table, &[Pid(1)]),
            vec![Pid(1), Pid(2), Pid(3), Pid(4)]
        );
    }

    #[test]
    fn an_agent_outside_every_known_app_has_no_owner() {
        let before = vec![p(9, 1, "claude", 0)];
        let after = vec![p(9, 1, "claude", 500)];
        let out = lit(&before, &after, &agents(), tick(), &[Pid(7)]);
        assert_eq!(out.len(), 1);
        assert!(out[0].owners.is_empty());
    }

    #[test]
    fn a_recycled_pid_does_not_inherit_the_old_process_cpu() {
        // A pid that is reused starts its CPU count again, so the stored
        // baseline is larger than the new process's total. Saturating keeps that
        // from wrapping into an enormous delta; assert the outcome rather than
        // just that nothing lit, so the test cannot pass merely because the
        // threshold happened to swallow it.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 9_000)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 5)];
        let agents: HashSet<Pid> = [Pid(2)].into_iter().collect();
        let now: HashMap<Pid, Duration> =
            [(Pid(1), Duration::ZERO), (Pid(2), Duration::from_millis(5))]
                .into_iter()
                .collect();
        let was: HashMap<Pid, Duration> = [
            (Pid(1), Duration::ZERO),
            (Pid(2), Duration::from_millis(9_000)),
        ]
        .into_iter()
        .collect();
        let mut children = HashMap::new();
        children.insert(Pid(1), vec![Pid(2)]);
        assert_eq!(
            burned(Pid(2), &children, &now, &was, &agents),
            Duration::ZERO,
            "a shrinking total is a new process, not negative work"
        );
        assert!(lit(&before, &after, &super::tests::agents(), tick(), &[Pid(1)]).is_empty());
    }

    #[test]
    fn an_exited_and_reused_pid_cannot_go_negative() {
        // pid reuse can make "now" smaller than "before"; that is not work.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 9_000)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 5)];
        assert!(lit(&before, &after, &agents(), tick(), &[Pid(1)]).is_empty());
    }

    #[test]
    fn a_parent_cycle_terminates() {
        let before = vec![p(2, 3, "claude", 0), p(3, 2, "fish", 0)];
        let after = vec![p(2, 3, "claude", 500), p(3, 2, "fish", 0)];
        let out = lit(&before, &after, &agents(), tick(), &[Pid(1)]);
        assert_eq!(out.len(), 1);
        assert!(out[0].owners.is_empty());
    }

    #[test]
    fn a_detached_agent_is_attributed_through_its_client() {
        // launchd -> server -> pane shell -> agent; app -> shell -> client.
        let tree = |cpu| {
            vec![
                p(10, 1, "mux-server", 0),
                p(11, 10, "fish", 0),
                p(12, 11, "claude", cpu),
                p(20, 1, "ghostty", 0),
                p(21, 20, "fish", 0),
                p(22, 21, "mux-client", 0),
            ]
        };
        let out = lit_through(
            &tree(0),
            &tree(500),
            &agents(),
            tick(),
            &[Pid(20)],
            |server| {
                assert_eq!(server, Pid(10));
                vec![Pid(22)]
            },
        );
        assert_eq!(out[0].owners, vec![Pid(20)]);
    }

    #[test]
    fn multiple_socket_clients_light_each_viewer_app() {
        let tree = |cpu| {
            vec![
                p(10, 1, "mux-server", 0),
                p(12, 10, "claude", cpu),
                p(20, 1, "ghostty", 0),
                p(21, 20, "mux-client", 0),
                p(30, 1, "terminal", 0),
                p(31, 30, "mux-client", 0),
            ]
        };
        let out = lit_through(
            &tree(0),
            &tree(500),
            &agents(),
            tick(),
            &[Pid(20), Pid(30)],
            |_| vec![Pid(31), Pid(21), Pid(21)],
        );
        assert_eq!(out[0].owners, vec![Pid(20), Pid(30)]);
        assert_eq!(by_owner(&out), HashMap::from([(Pid(20), 1), (Pid(30), 1)]));
    }

    #[test]
    fn no_socket_client_leaves_a_detached_agent_ownerless() {
        let before = vec![p(10, 1, "mux-server", 0), p(12, 10, "claude", 0)];
        let after = vec![p(10, 1, "mux-server", 0), p(12, 10, "claude", 500)];
        let out = lit_through(&before, &after, &agents(), tick(), &[], |_| Vec::new());
        assert_eq!(out.len(), 1);
        assert!(out[0].owners.is_empty());
    }

    #[test]
    fn an_attached_agent_does_not_consult_socket_topology() {
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 0)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 500)];
        let out = lit_through(&before, &after, &agents(), tick(), &[Pid(1)], |_| {
            panic!("an attached chain needs no socket lookup")
        });
        assert_eq!(out[0].owners, vec![Pid(1)]);
    }
}
