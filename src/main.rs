fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag) if flag == input::WATCHDOG_FLAG => {
            match args.next().and_then(|p| p.parse().ok()) {
                Some(parent) => input::watchdog_main(parent),
                None => std::process::exit(2),
            }
        }
        Some("--suppress-and-hang") if cfg!(debug_assertions) => {
            let suppression = input::Suppression::engage();
            println!(
                "{}",
                if suppression.is_some() {
                    "engaged"
                } else {
                    "engage-failed"
                }
            );
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        _ => {}
    }

    if !ax::trusted() {
        println!("accessibility: required — approve the system prompt, then relaunch");
        ax::request_trust();
        return;
    }
    if !capture::permitted() && !capture::request_permission() {
        println!("screen recording: not granted — previews and titles disabled");
    }

    let ws = match winsrv::WindowServer::connect() {
        Ok(ws) => ws,
        Err(missing) => {
            println!("skylight: unavailable — {}", missing.join(", "));
            return;
        }
    };
    let spaces = ws.spaces();
    let current = spaces
        .iter()
        .find(|s| s.current)
        .map(|s| format!(" (current {})", s.id))
        .unwrap_or_default();
    println!("spaces: {}{current}", spaces.len());

    let ids: Vec<u64> = spaces.iter().map(|s| s.id).collect();
    let windows = ws.windows(&ids);
    println!("windows: {}", windows.len());
    for w in &windows {
        println!(
            "  {:>6}  pid {:>6}  lvl {:>5}  tags {:#018x}  {:<24}  {}",
            w.wid,
            w.pid,
            w.level,
            w.tags,
            w.app.as_deref().unwrap_or("?"),
            w.title.as_deref().unwrap_or(""),
        );
    }
}
