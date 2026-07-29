//! What Lantern sees, through the same constants the app uses. Any number here
//! that drifts from `src/lantern.rs` makes this a different detector wearing the
//! same name, so the thresholds and window are imported rather than restated.
//!
//! `cargo run -p proctable --example probe`

use std::time::Instant;

const TERMINALS: [&str; 6] = [
    "ghostty",
    "iterm",
    "terminal",
    "alacritty",
    "wezterm",
    "kitty",
];

/// Kept in step with `lantern::DEFAULT_BINARIES` by the test below.
const BINARIES: [&str; 6] = [
    "claude",
    "cursor-agent",
    "codex",
    "aider",
    "gemini",
    "opencode",
];
const WINDOW: std::time::Duration = std::time::Duration::from_millis(2000);
const THRESHOLD: std::time::Duration = std::time::Duration::from_millis(150);

fn main() {
    let binaries: Vec<String> = BINARIES.iter().map(|s| (*s).to_string()).collect();

    let sample = |roots: &[i32]| {
        let mut table = proctable::table();
        let under = model::descendants(&table, roots);
        proctable::detail(&mut table, &under);
        table
    };

    let seed = proctable::table();
    let roots: Vec<i32> = seed
        .iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            TERMINALS.iter().any(|t| n.contains(t))
        })
        .map(|p| p.pid)
        .collect();

    let started = Instant::now();
    let before = sample(&roots);
    std::thread::sleep(WINDOW);
    let after = sample(&roots);

    let lit = model::lit(&before, &after, &binaries, THRESHOLD, &roots);
    println!("processes       : {}", after.len());
    println!("terminal roots  : {roots:?}");
    println!("window          : {WINDOW:?}   threshold: {THRESHOLD:?}");
    println!("elapsed         : {:?}", started.elapsed());
    for proc in after.iter().filter(|p| binaries.contains(&p.name)) {
        let burn = model::burn(&before, &after, proc.pid, &binaries);
        println!(
            "  pid {:>6} {:<14} {:>8.1} ms  {}",
            proc.pid,
            proc.name,
            burn.as_secs_f64() * 1000.0,
            if lit.iter().any(|l| l.agent == proc.pid) {
                "WORKING"
            } else {
                "idle"
            }
        );
    }
    for (app, n) in model::by_owner(&lit) {
        println!("app {app} has {n} agent(s) working");
    }
}
