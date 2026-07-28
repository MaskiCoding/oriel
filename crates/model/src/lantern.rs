//! Lantern: which windows have an agent burning work inside them.
//!
//! Agent CLIs announce their state in the window title, but that title lies in
//! both directions — the same braille spinner means "waiting for input" and
//! "running a tool", and a covered window stops redrawing so the last title can
//! be minutes stale. CPU time cannot lie the same way: a waiting agent burns
//! none, a working one burns some, and neither depends on being on screen.
//!
//! Work counts across the agent's whole subtree, because an agent running a
//! tool sits idle while the tool burns — that is the exact case the title
//! spinner got wrong.

use std::collections::HashMap;
use std::time::Duration;

/// One process as the kernel reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    /// CPU time burned since this process started.
    pub cpu: Duration,
}

/// An agent found to be working, and the app whose windows should light up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lit {
    pub agent: i32,
    pub name: String,
    /// The ancestor from `roots` this agent runs under, if any.
    pub owner: Option<i32>,
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
    roots: &[i32],
) -> Vec<Lit> {
    let was: HashMap<i32, Duration> = before.iter().map(|p| (p.pid, p.cpu)).collect();
    let parent: HashMap<i32, i32> = after.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for p in after {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let now: HashMap<i32, Duration> = after.iter().map(|p| (p.pid, p.cpu)).collect();

    let is_agent: Vec<i32> = after
        .iter()
        .filter(|p| binaries.iter().any(|b| b == &p.name))
        .map(|p| p.pid)
        .collect();

    let mut out: Vec<Lit> = after
        .iter()
        .filter(|p| is_agent.contains(&p.pid))
        .filter(|p| burned(p.pid, &children, &now, &was, &is_agent) > threshold)
        .map(|p| Lit {
            agent: p.pid,
            name: p.name.clone(),
            owner: owner_of(p.pid, &parent, roots),
        })
        .collect();
    out.sort_by_key(|l| l.agent);
    out
}

/// CPU burned by `pid` and the tools under it. A process that appeared since
/// the last sample counts all of its time: a tool that spawned and ran is work.
///
/// The walk stops at any *other* agent, so an agent that merely sits waiting on
/// a nested one is dark while the nested one is lit. Without this an orchestrator
/// and each of its workers would all claim the same work.
fn burned(
    pid: i32,
    children: &HashMap<i32, Vec<i32>>,
    now: &HashMap<i32, Duration>,
    was: &HashMap<i32, Duration>,
    agents: &[i32],
) -> Duration {
    let mut total = Duration::ZERO;
    let mut stack = vec![pid];
    let mut seen = 0usize;
    while let Some(p) = stack.pop() {
        // A corrupt ppid chain could cycle; the table is the only bound we need.
        seen += 1;
        if seen > now.len() {
            break;
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
fn owner_of(pid: i32, parent: &HashMap<i32, i32>, roots: &[i32]) -> Option<i32> {
    let mut cur = pid;
    for _ in 0..parent.len() {
        if roots.contains(&cur) {
            return Some(cur);
        }
        match parent.get(&cur) {
            Some(&next) if next != cur && next > 0 => cur = next,
            _ => return None,
        }
    }
    None
}

/// Every process running under any of `roots`, roots included. Callers use this
/// to resolve true executable names for the few processes that could be agents
/// instead of the whole table, which costs a syscall each.
pub fn descendants(table: &[Proc], roots: &[i32]) -> Vec<i32> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for p in table {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let mut out = Vec::new();
    let mut stack: Vec<i32> = roots.to_vec();
    while let Some(pid) = stack.pop() {
        if out.contains(&pid) || out.len() > table.len() {
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
pub fn by_owner(lit: &[Lit]) -> HashMap<i32, usize> {
    let mut out: HashMap<i32, usize> = HashMap::new();
    for l in lit.iter().filter_map(|l| l.owner) {
        *out.entry(l).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, parent: i32, name: &str, ms: u64) -> Proc {
        Proc {
            pid,
            ppid: parent,
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
        let out = lit(&before, &after, &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].agent, 2);
        assert_eq!(out[0].owner, Some(1));
    }

    #[test]
    fn an_agent_waiting_for_input_is_dark() {
        // The measured idle case: a parked agent burns exactly nothing.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 100)];
        assert!(lit(&before, &after, &agents(), tick(), &[1]).is_empty());
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
        let out = lit(&before, &after, &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1, "subtree work must light the agent");
        assert_eq!(out[0].agent, 2);
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
        let out = lit(&table(0), &table(400), &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1, "only the nested worker is doing the work");
        assert_eq!(out[0].agent, 3);
        assert_eq!(by_owner(&out).get(&1), Some(&1));
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
        let out = lit(&table(0), &table(400), &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].agent, 2);
    }

    #[test]
    fn a_busy_non_agent_is_ignored() {
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "cargo", 100)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "cargo", 9_000)];
        assert!(lit(&before, &after, &agents(), tick(), &[1]).is_empty());
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
        let out = lit(&chain(100), &chain(500), &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].owner,
            Some(1),
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
        let out = lit(&both(0, 0), &both(300, 300), &agents(), tick(), &[1]);
        assert_eq!(out.len(), 2);
        assert_eq!(by_owner(&out).get(&1), Some(&2));
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
        assert_eq!(descendants(&table, &[1]), vec![1, 2, 3, 4]);
    }

    #[test]
    fn an_agent_outside_every_known_app_has_no_owner() {
        let before = vec![p(9, 1, "claude", 0)];
        let after = vec![p(9, 1, "claude", 500)];
        let out = lit(&before, &after, &agents(), tick(), &[7]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].owner, None);
    }

    #[test]
    fn an_exited_and_reused_pid_cannot_go_negative() {
        // pid reuse can make "now" smaller than "before"; that is not work.
        let before = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 9_000)];
        let after = vec![p(1, 0, "ghostty", 0), p(2, 1, "claude", 5)];
        assert!(lit(&before, &after, &agents(), tick(), &[1]).is_empty());
    }

    #[test]
    fn a_parent_cycle_terminates() {
        let before = vec![p(2, 3, "claude", 0), p(3, 2, "fish", 0)];
        let after = vec![p(2, 3, "claude", 500), p(3, 2, "fish", 0)];
        let out = lit(&before, &after, &agents(), tick(), &[1]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].owner, None);
    }
}
