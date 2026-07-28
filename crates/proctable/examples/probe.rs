//! Ground truth for Lantern: sample the process table twice and report which
//! agents were working, plus what it cost. `cargo run -p proctable --example probe`

use std::time::{Duration, Instant};

const TERMINALS: [&str; 6] = [
    "ghostty",
    "iterm",
    "terminal",
    "alacritty",
    "wezterm",
    "kitty",
];

fn main() {
    let binaries: Vec<String> = ["claude", "cursor-agent", "codex", "aider"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

    let t0 = Instant::now();
    let mut before = proctable::table();
    let roots: Vec<i32> = before
        .iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            TERMINALS.iter().any(|t| n.contains(t))
        })
        .map(|p| p.pid)
        .collect();
    let under = model::descendants(&before, &roots);
    proctable::detail(&mut before, &under);
    let cost = t0.elapsed();

    std::thread::sleep(Duration::from_millis(1000));

    let t1 = Instant::now();
    let mut after = proctable::table();
    let under2 = model::descendants(&after, &roots);
    proctable::detail(&mut after, &under2);
    let cost2 = t1.elapsed();

    for p in after.iter().filter(|p| under2.contains(&p.pid)) {
        println!(
            "    under: pid {:>6} ppid {:>6} {:<20} cpu {:?}",
            p.pid, p.ppid, p.name, p.cpu
        );
    }

    let lit = model::lit(
        &before,
        &after,
        &binaries,
        Duration::from_millis(10),
        &roots,
    );

    println!("processes        : {}", after.len());
    println!("terminal roots   : {roots:?}");
    println!("under terminals  : {}", under2.len());
    println!("cost per sample  : {cost:?} then {cost2:?}");
    println!("agents lit       : {}", lit.len());
    for l in &lit {
        println!(
            "  pid {:>6}  {:<14} owner {:?}  WORKING",
            l.agent, l.name, l.owner
        );
    }
    for p in after
        .iter()
        .filter(|p| binaries.contains(&p.name))
        .filter(|p| !lit.iter().any(|l| l.agent == p.pid))
    {
        println!("  pid {:>6}  {:<14} dark (waiting)", p.pid, p.name);
    }
    for (app, n) in model::by_owner(&lit) {
        println!("app {app} has {n} agent(s) working");
    }
}
